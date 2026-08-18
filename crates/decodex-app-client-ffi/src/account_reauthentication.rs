use std::{
	fs::{self, File},
	io::Read,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
		unix::{
			fs::{MetadataExt as _, PermissionsExt as _},
			process::CommandExt as _,
		},
	},
	path::{Path, PathBuf},
	process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, RecvTimeoutError, Sender},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant},
};

use decodex_protocol::{
	AccountClient, AccountCommandRejectionDto, AccountCommandResponse, ClientFailure,
	ClientProfile, CommandError, CommandPayload, EntityId, EntityRevision, IdempotencyKey,
	ResultPayload, WireText,
};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

const DEVICE_LOGIN_URL: &str = "https://auth.openai.com/codex/device";
const FILE_AUTH_STORE_CONFIG: &str = r#"cli_auth_credentials_store="file""#;
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(40);
const MAX_LOGIN_OUTPUT_BYTES: usize = 64 * 1024;
const INSTALL_DISPATCH_ATTEMPTS: usize = 3;
const INSTALL_REPLAY_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Failure {
	CodexUnavailable,
	LoginFailed,
	LoginTimedOut,
	AccountMismatch,
	AccountChanged,
	AccountUnavailable,
	ProviderAlreadyEnrolled,
	RecoveryChanged,
	CredentialStoreUnavailable,
	ServiceUnavailable,
	OutcomeUnknown,
	SessionNotFound,
	Busy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
	OpeningBrowser,
	RequestingCode,
	WaitingForBrowser,
	Installing,
	Completed,
	Failed,
	Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoginMethod {
	BrowserRedirect,
	DeviceCode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Prompt {
	verification_url: &'static str,
	user_code: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Status {
	session_id: String,
	state: State,
	#[serde(skip_serializing_if = "Option::is_none")]
	prompt: Option<Prompt>,
	#[serde(skip_serializing_if = "Option::is_none")]
	failure: Option<Failure>,
}

impl Status {
	fn opening_browser(session_id: String) -> Self {
		Self { session_id, state: State::OpeningBrowser, prompt: None, failure: None }
	}

	fn requesting_code(session_id: String) -> Self {
		Self { session_id, state: State::RequestingCode, prompt: None, failure: None }
	}

	fn waiting_for_browser(session_id: String, prompt: Option<Prompt>) -> Self {
		Self { session_id, state: State::WaitingForBrowser, prompt, failure: None }
	}

	fn installing(session_id: String) -> Self {
		Self { session_id, state: State::Installing, prompt: None, failure: None }
	}

	fn completed(session_id: String) -> Self {
		Self { session_id, state: State::Completed, prompt: None, failure: None }
	}

	fn failed(session_id: String, failure: Failure) -> Self {
		Self { session_id, state: State::Failed, prompt: None, failure: Some(failure) }
	}

	fn cancelled(session_id: String) -> Self {
		Self { session_id, state: State::Cancelled, prompt: None, failure: None }
	}

	fn is_terminal(&self) -> bool {
		matches!(self.state, State::Completed | State::Failed | State::Cancelled)
	}
}

pub(crate) enum InstallMode {
	Reauthenticate {
		expected_revision: EntityRevision,
		recovery_operation_id: Option<EntityId>,
	},
	Enroll {
		enabled: bool,
	},
}

pub(crate) struct Start {
	pub session_id: String,
	pub operation_id: EntityId,
	pub account_id: EntityId,
	pub idempotency_key: IdempotencyKey,
	pub codex_bin: PathBuf,
	pub login_method: LoginMethod,
	pub install_mode: InstallMode,
}

struct Shared {
	status: Mutex<Status>,
	cancel_requested: AtomicBool,
}

impl Shared {
	fn new(session_id: String, login_method: LoginMethod) -> Self {
		let status = match login_method {
			LoginMethod::BrowserRedirect => Status::opening_browser(session_id),
			LoginMethod::DeviceCode => Status::requesting_code(session_id),
		};
		Self {
			status: Mutex::new(status),
			cancel_requested: AtomicBool::new(false),
		}
	}

	fn status(&self) -> Status {
		match self.status.lock() {
			Ok(status) => status.clone(),
			Err(poisoned) => poisoned.into_inner().clone(),
		}
	}

	fn set_status(&self, status: Status) {
		match self.status.lock() {
			Ok(mut current) => *current = status,
			Err(poisoned) => *poisoned.into_inner() = status,
		}
	}
}

struct Session {
	session_id: String,
	shared: Arc<Shared>,
	worker: Option<JoinHandle<()>>,
}

pub(crate) struct Manager {
	session: Mutex<Option<Session>>,
	closed: AtomicBool,
}

impl Default for Manager {
	fn default() -> Self {
		Self { session: Mutex::new(None), closed: AtomicBool::new(false) }
	}
}

impl Manager {
	pub(crate) fn start(&self, start: Start, profile: ClientProfile, runtime: Handle) -> Status {
		let mut slot = self.lock_session();
		if self.closed.load(Ordering::Acquire) {
			return Status::failed(start.session_id, Failure::ServiceUnavailable);
		}
		if let Some(session) = slot.as_ref() {
			if session.session_id == start.session_id {
				return session.shared.status();
			}
			if !session.shared.status().is_terminal() {
				return Status::failed(start.session_id, Failure::Busy);
			}
		}
		let shared = Arc::new(Shared::new(start.session_id.clone(), start.login_method));
		let worker_shared = Arc::clone(&shared);
		let worker = match thread::Builder::new()
			.name("decodex-account-login".to_owned())
			.spawn(move || run_login(worker_shared, start, profile, runtime))
		{
			Ok(worker) => Some(worker),
			Err(_) => {
				shared.set_status(Status::failed(
					shared.status().session_id,
					Failure::ServiceUnavailable,
				));
				None
			},
		};
		let status = shared.status();
		*slot = Some(Session { session_id: status.session_id.clone(), shared, worker });
		status
	}

	pub(crate) fn poll(&self, session_id: &str) -> Status {
		let slot = self.lock_session();
		slot.as_ref().filter(|session| session.session_id == session_id).map_or_else(
			|| Status::failed(session_id.to_owned(), Failure::SessionNotFound),
			|session| session.shared.status(),
		)
	}

	pub(crate) fn cancel(&self, session_id: &str) -> Status {
		let (shared, worker) = {
			let mut slot = self.lock_session();
			let Some(session) = slot.as_ref().filter(|session| session.session_id == session_id)
			else {
				return Status::failed(session_id.to_owned(), Failure::SessionNotFound);
			};
			let shared = Arc::clone(&session.shared);
			shared.cancel_requested.store(true, Ordering::Release);
			let worker = slot.as_mut().and_then(|session| session.worker.take());
			(shared, worker)
		};
		if let Some(worker) = worker
			&& worker.join().is_err()
		{
			shared.set_status(Status::failed(session_id.to_owned(), Failure::ServiceUnavailable));
		}
		wait_for_terminal(&shared)
	}

	pub(crate) fn shutdown(&self) {
		self.closed.store(true, Ordering::Release);
		let session = {
			let mut slot = self.lock_session();
			slot.as_mut().map(|session| {
				session.shared.cancel_requested.store(true, Ordering::Release);
				(Arc::clone(&session.shared), session.worker.take())
			})
		};
		if let Some((shared, worker)) = session {
			if let Some(worker) = worker
				&& worker.join().is_err()
			{
				shared.set_status(Status::failed(
					shared.status().session_id,
					Failure::ServiceUnavailable,
				));
			}
			wait_for_terminal(&shared);
		}
	}

	fn lock_session(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
		match self.session.lock() {
			Ok(session) => session,
			Err(poisoned) => poisoned.into_inner(),
		}
	}
}

fn wait_for_terminal(shared: &Shared) -> Status {
	loop {
		let status = shared.status();
		if status.is_terminal() {
			return status;
		}
		thread::sleep(Duration::from_millis(10));
	}
}

impl Drop for Manager {
	fn drop(&mut self) {
		self.shutdown();
	}
}

fn run_login(shared: Arc<Shared>, start: Start, profile: ClientProfile, runtime: Handle) {
	let status = run_login_session(&shared, start, profile, runtime);
	shared.set_status(status);
}

fn run_login_session(
	shared: &Shared,
	start: Start,
	profile: ClientProfile,
	runtime: Handle,
) -> Status {
	let session_id = start.session_id.clone();
	let codex_bin = match canonical_executable(&start.codex_bin) {
		Ok(codex_bin) => codex_bin,
		Err(failure) => return Status::failed(session_id, failure),
	};
	let mut login_home = match LoginHome::create(&start.session_id) {
		Ok(home) => home,
		Err(_) => return Status::failed(session_id, Failure::ServiceUnavailable),
	};
	let status = run_login_in_home(shared, start, profile, runtime, codex_bin, login_home.path());
	finalize_login_status(&mut login_home, session_id, status)
}

fn finalize_login_status(login_home: &mut LoginHome, session_id: String, status: Status) -> Status {
	if login_home.cleanup().is_ok() {
		return status;
	}
	let failure =
		if status.state == State::Completed || status.failure == Some(Failure::OutcomeUnknown) {
			Failure::OutcomeUnknown
		} else {
			Failure::ServiceUnavailable
		};
	Status::failed(session_id, failure)
}

fn run_login_in_home(
	shared: &Shared,
	start: Start,
	profile: ClientProfile,
	runtime: Handle,
	codex_bin: PathBuf,
	login_home: &Path,
) -> Status {
	let session_id = start.session_id.clone();
	let login_method = start.login_method;
	let spawned = match spawn_login_process(
		login_command(&codex_bin, login_home, login_method),
		login_method,
	) {
		Ok(spawned) => spawned,
		Err(failure) => return Status::failed(session_id, failure),
	};
	let SpawnedLoginProcess { mut child, stdout, stderr } = spawned;
	if login_method == LoginMethod::BrowserRedirect {
		shared.set_status(Status::waiting_for_browser(session_id.clone(), None));
	}
	let (sender, receiver) = mpsc::channel();
	let stdout_reader = match spawn_reader(stdout, sender.clone()) {
		Ok(reader) => reader,
		Err(()) => {
			terminate_child(&mut child);
			return Status::failed(session_id, Failure::ServiceUnavailable);
		},
	};
	let stderr_reader = match spawn_reader(stderr, sender) {
		Ok(reader) => reader,
		Err(()) => {
			terminate_child(&mut child);
			let _ = stdout_reader.join();
			return Status::failed(session_id, Failure::ServiceUnavailable);
		},
	};
	let output = match collect_login_child(
		shared,
		&mut child,
		receiver,
		stdout_reader,
		stderr_reader,
		&session_id,
		login_method,
	) {
		Ok(output) => output,
		Err(status) => return status,
	};
	if output.reader_failed
		|| !output.exit.success()
		|| (login_method == LoginMethod::DeviceCode && !output.prompt_published)
	{
		return Status::failed(session_id, Failure::LoginFailed);
	}
	if shared.cancel_requested.load(Ordering::Acquire) {
		return Status::cancelled(session_id);
	}
	let auth_path = match canonical_auth_file(login_home) {
		Ok(path) => path,
		Err(_) => return Status::failed(session_id, Failure::LoginFailed),
	};
	shared.set_status(Status::installing(session_id.clone()));
	let outcome = runtime.block_on(install_credential(profile, start, auth_path));
	match outcome {
		Ok(()) => Status::completed(session_id),
		Err(failure) => Status::failed(session_id, failure),
	}
}

fn login_command(codex_bin: &Path, login_home: &Path, login_method: LoginMethod) -> Command {
	let mut command = Command::new(codex_bin);
	command.arg("login");
	if login_method == LoginMethod::DeviceCode {
		command.arg("--device-auth");
	}
	command
		.arg("-c")
		.arg(FILE_AUTH_STORE_CONFIG)
		.current_dir(login_home)
		.env_remove("OPENAI_API_KEY")
		.env_remove("CODEX_API_KEY")
		.env_remove("CODEX_ACCESS_TOKEN")
		.env_remove("CHATGPT_ACCESS_TOKEN")
		.env_remove("CODEX_HOME")
		.env_remove("CODEX_SQLITE_HOME")
		.env("CODEX_HOME", login_home)
		.env("CODEX_SQLITE_HOME", login_home)
		.stdin(Stdio::null())
		.stderr(Stdio::piped())
		.process_group(0);
	command
}

struct SpawnedLoginProcess {
	child: Child,
	stdout: LoginStdout,
	stderr: ChildStderr,
}

enum LoginStdout {
	Pipe(ChildStdout),
	PseudoTerminal(PseudoTerminalReader),
}

impl Read for LoginStdout {
	fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
		match self {
			Self::Pipe(reader) => reader.read(buffer),
			Self::PseudoTerminal(reader) => reader.read(buffer),
		}
	}
}

struct PseudoTerminalReader(File);

impl Read for PseudoTerminalReader {
	fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
		match self.0.read(buffer) {
			Err(error) if error.raw_os_error() == Some(libc::EIO) => Ok(0),
			result => result,
		}
	}
}

struct PseudoTerminal {
	master: OwnedFd,
	slave: OwnedFd,
}

impl PseudoTerminal {
	fn open() -> Result<Self, ()> {
		let mut master = -1;
		let mut slave = -1;
		// SAFETY: `openpty` initializes both output descriptors on success. The
		// optional name, termios, and window-size pointers are intentionally null.
		if unsafe {
			libc::openpty(
				&raw mut master,
				&raw mut slave,
				std::ptr::null_mut(),
				std::ptr::null_mut(),
				std::ptr::null_mut(),
			)
		} == -1
		{
			return Err(());
		}
		// SAFETY: A successful `openpty` returned two new descriptors owned by
		// this process. `OwnedFd` closes every subsequent error and drop path.
		let master = unsafe { OwnedFd::from_raw_fd(master) };
		// SAFETY: Same ownership proof as the master descriptor above.
		let slave = unsafe { OwnedFd::from_raw_fd(slave) };
		set_close_on_exec(&master)?;
		set_close_on_exec(&slave)?;
		// SAFETY: `slave` is a valid open PTY descriptor owned by this process.
		if unsafe { libc::fchmod(slave.as_raw_fd(), 0o600) } == -1 {
			return Err(());
		}
		let metadata = fd_metadata(&slave)?;
		if metadata.st_uid != unsafe { libc::geteuid() } || metadata.st_mode & 0o077 != 0 {
			return Err(());
		}
		Ok(Self { master, slave })
	}
}

fn set_close_on_exec(fd: &OwnedFd) -> Result<(), ()> {
	// SAFETY: `fd` owns a valid open descriptor for both `fcntl` operations.
	let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
	if flags == -1 {
		return Err(());
	}
	// SAFETY: The same descriptor remains open, and `F_SETFD` accepts the
	// existing flags with `FD_CLOEXEC` added.
	if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
		return Err(());
	}
	Ok(())
}

fn fd_metadata(fd: &OwnedFd) -> Result<libc::stat, ()> {
	let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
	// SAFETY: `fd` is valid and `metadata` points to writable storage for one
	// `stat` value. A successful call initializes that value completely.
	if unsafe { libc::fstat(fd.as_raw_fd(), metadata.as_mut_ptr()) } == -1 {
		return Err(());
	}
	// SAFETY: The successful `fstat` above initialized the value.
	Ok(unsafe { metadata.assume_init() })
}

fn spawn_login_process(
	mut command: Command,
	login_method: LoginMethod,
) -> Result<SpawnedLoginProcess, Failure> {
	let mut pseudo_terminal_reader = None;
	match login_method {
		LoginMethod::BrowserRedirect => {
			command.stdout(Stdio::piped());
		},
		LoginMethod::DeviceCode => {
			let PseudoTerminal { master, slave } =
				PseudoTerminal::open().map_err(|()| Failure::ServiceUnavailable)?;
			command.stdout(Stdio::from(slave));
			pseudo_terminal_reader = Some(PseudoTerminalReader(File::from(master)));
		},
	};
	let mut child = command.spawn().map_err(|_| Failure::CodexUnavailable)?;
	let stdout = match pseudo_terminal_reader {
		Some(reader) => LoginStdout::PseudoTerminal(reader),
		None => {
			let Some(stdout) = child.stdout.take() else {
				terminate_child(&mut child);
				return Err(Failure::LoginFailed);
			};
			LoginStdout::Pipe(stdout)
		},
	};
	let Some(stderr) = child.stderr.take() else {
		terminate_child(&mut child);
		return Err(Failure::LoginFailed);
	};
	Ok(SpawnedLoginProcess { child, stdout, stderr })
}

struct LoginChildOutput {
	exit: ExitStatus,
	reader_failed: bool,
	prompt_published: bool,
}

fn collect_login_child(
	shared: &Shared,
	child: &mut Child,
	receiver: Receiver<PipeEvent>,
	stdout_reader: JoinHandle<()>,
	stderr_reader: JoinHandle<()>,
	session_id: &str,
	login_method: LoginMethod,
) -> Result<LoginChildOutput, Status> {
	let deadline = Instant::now() + LOGIN_TIMEOUT;
	let mut output = Vec::new();
	let mut reader_failed = false;
	let mut prompt_published = false;
	let mut open_readers = 2_u8;
	let mut exit = None;
	loop {
		if shared.cancel_requested.load(Ordering::Acquire) {
			terminate_child(child);
			join_readers(stdout_reader, stderr_reader);
			return Err(Status::cancelled(session_id.to_owned()));
		}
		if Instant::now() >= deadline {
			terminate_child(child);
			join_readers(stdout_reader, stderr_reader);
			return Err(Status::failed(session_id.to_owned(), Failure::LoginTimedOut));
		}
		match receiver.recv_timeout(CHILD_POLL_INTERVAL) {
			Ok(PipeEvent::Bytes(bytes)) => {
				if append_output(&mut output, &bytes).is_err() {
					terminate_child(child);
					join_readers(stdout_reader, stderr_reader);
					return Err(Status::failed(session_id.to_owned(), Failure::LoginFailed));
				}
				publish_device_prompt(
					shared,
					&output,
					login_method,
					session_id,
					&mut prompt_published,
				);
			},
			Ok(PipeEvent::Closed { failed }) => {
				reader_failed |= failed;
				open_readers = open_readers.saturating_sub(1);
			},
			Err(RecvTimeoutError::Timeout) => {},
			Err(RecvTimeoutError::Disconnected) => {
				reader_failed |= open_readers != 0;
				open_readers = 0;
				thread::sleep(CHILD_POLL_INTERVAL);
			},
		}
		if exit.is_none() {
			match child.try_wait() {
				Ok(Some(status)) => exit = Some(status),
				Ok(None) => {},
				Err(_) => {
					terminate_child(child);
					join_readers(stdout_reader, stderr_reader);
					return Err(Status::failed(session_id.to_owned(), Failure::LoginFailed));
				},
			}
		}
		if let Some(exit) = exit {
			// The login leader can exit after it writes auth.json while one of
			// its descendants still owns an inherited output pipe. Reap that
			// exact process group so a successful browser callback does not wait
			// for the full login timeout before credential installation.
			terminate_child(child);
			join_readers(stdout_reader, stderr_reader);
			for event in receiver.try_iter() {
				match event {
					PipeEvent::Bytes(bytes) => {
						if append_output(&mut output, &bytes).is_err() {
							return Err(Status::failed(
								session_id.to_owned(),
								Failure::LoginFailed,
							));
						}
						publish_device_prompt(
							shared,
							&output,
							login_method,
							session_id,
							&mut prompt_published,
						);
					},
					PipeEvent::Closed { failed } => reader_failed |= failed,
				}
			}
			return Ok(LoginChildOutput { exit, reader_failed, prompt_published });
		}
	}
}

fn publish_device_prompt(
	shared: &Shared,
	output: &[u8],
	login_method: LoginMethod,
	session_id: &str,
	prompt_published: &mut bool,
) {
	if login_method != LoginMethod::DeviceCode || *prompt_published {
		return;
	}
	if let Some(prompt) = parse_device_prompt(output) {
		shared.set_status(Status::waiting_for_browser(session_id.to_owned(), Some(prompt)));
		*prompt_published = true;
	}
}

async fn install_credential(
	profile: ClientProfile,
	start: Start,
	auth_path: PathBuf,
) -> Result<(), Failure> {
	let source_descriptor = WireText::new(auth_path.to_string_lossy().into_owned())
		.map_err(|_| Failure::LoginFailed)?;
	let (payload, expected_revision) = install_command(&start, source_descriptor);
	for attempt in 0..INSTALL_DISPATCH_ATTEMPTS {
		let response = AccountClient::new(profile.clone())
			.execute(payload.clone(), expected_revision, start.idempotency_key.clone())
			.await
			.map_err(map_client_failure)?;
		match response {
			AccountCommandResponse::Applied { result, .. } => match result.as_ref() {
				ResultPayload::AccountChanged { account }
					if account.account_id == start.account_id =>
					return Ok(()),
				_ => return Err(Failure::ServiceUnavailable),
			},
			AccountCommandResponse::Rejected { error } => return Err(map_command_error(&error)),
			AccountCommandResponse::PotentiallyDispatched { .. }
				if attempt + 1 < INSTALL_DISPATCH_ATTEMPTS =>
			{
				tokio::time::sleep(INSTALL_REPLAY_DELAY).await;
			},
			AccountCommandResponse::PotentiallyDispatched { .. } =>
				return Err(Failure::OutcomeUnknown),
		}
	}
	Err(Failure::OutcomeUnknown)
}

fn install_command(
	start: &Start,
	source_descriptor: WireText,
) -> (CommandPayload, Option<EntityRevision>) {
	match &start.install_mode {
		InstallMode::Reauthenticate { expected_revision, recovery_operation_id } => (
			CommandPayload::ReauthenticateAccountFromCredentialFile {
				operation_id: start.operation_id.clone(),
				account_id: start.account_id.clone(),
				recovery_operation_id: recovery_operation_id.clone(),
				source_descriptor,
			},
			Some(*expected_revision),
		),
		InstallMode::Enroll { enabled } => (
			CommandPayload::EnrollAccountFromCredentialFile {
				operation_id: start.operation_id.clone(),
				account_id: start.account_id.clone(),
				enabled: *enabled,
				source_descriptor,
			},
			None,
		),
	}
}

fn map_client_failure(_failure: ClientFailure) -> Failure {
	Failure::ServiceUnavailable
}

fn map_command_error(error: &CommandError) -> Failure {
	match error {
		CommandError::ExpectedRevisionMismatch { .. } => Failure::AccountChanged,
		CommandError::AccountCommandRejected { rejection, .. } => match rejection {
			AccountCommandRejectionDto::ProviderMismatch => Failure::AccountMismatch,
			AccountCommandRejectionDto::ProviderAlreadyEnrolled => Failure::ProviderAlreadyEnrolled,
			AccountCommandRejectionDto::StaleAccount => Failure::AccountChanged,
			AccountCommandRejectionDto::CredentialStoreUnavailable =>
				Failure::CredentialStoreUnavailable,
			AccountCommandRejectionDto::AccountNotFound
			| AccountCommandRejectionDto::CredentialAbsent
			| AccountCommandRejectionDto::LifecycleUnready
			| AccountCommandRejectionDto::OperationUnsettled
			| AccountCommandRejectionDto::InvalidRequest
			| AccountCommandRejectionDto::AccountInUse
			| AccountCommandRejectionDto::RoutingOrderInvalid => Failure::AccountUnavailable,
			AccountCommandRejectionDto::OperationNotFound
			| AccountCommandRejectionDto::ManualRecoveryRequired => Failure::RecoveryChanged,
			AccountCommandRejectionDto::StaleRoutingControl => Failure::AccountChanged,
		},
		CommandError::IdempotencyConflict
		| CommandError::IdempotencyCapacityExceeded { .. }
		| CommandError::ApplicationUnavailable { .. }
		| CommandError::QuickTaskUnavailable { .. }
		| CommandError::QuickTaskRecoveryRequired { .. }
		| CommandError::AcceptanceUnknown => Failure::ServiceUnavailable,
	}
}

fn canonical_executable(path: &Path) -> Result<PathBuf, Failure> {
	if !path.is_absolute() {
		return Err(Failure::CodexUnavailable);
	}
	let canonical = fs::canonicalize(path).map_err(|_| Failure::CodexUnavailable)?;
	if canonical != path {
		return Err(Failure::CodexUnavailable);
	}
	let metadata = fs::symlink_metadata(path).map_err(|_| Failure::CodexUnavailable)?;
	if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
		return Err(Failure::CodexUnavailable);
	}
	Ok(canonical)
}

fn canonical_auth_file(login_home: &Path) -> Result<PathBuf, ()> {
	let expected = login_home.join("auth.json");
	let canonical = fs::canonicalize(&expected).map_err(|_| ())?;
	if canonical != expected {
		return Err(());
	}
	let metadata = fs::symlink_metadata(&expected).map_err(|_| ())?;
	if !metadata.file_type().is_file()
		|| metadata.uid() != unsafe { libc::geteuid() }
		|| metadata.permissions().mode() & 0o077 != 0
	{
		return Err(());
	}
	Ok(canonical)
}

enum PipeEvent {
	Bytes(Vec<u8>),
	Closed { failed: bool },
}

fn spawn_reader(
	mut reader: impl Read + Send + 'static,
	sender: Sender<PipeEvent>,
) -> Result<JoinHandle<()>, ()> {
	thread::Builder::new()
		.name("decodex-account-login-pipe".to_owned())
		.spawn(move || {
			let mut buffer = [0_u8; 4_096];
			loop {
				match reader.read(&mut buffer) {
					Ok(0) => {
						let _ = sender.send(PipeEvent::Closed { failed: false });
						return;
					},
					Ok(length) =>
						if sender.send(PipeEvent::Bytes(buffer[..length].to_vec())).is_err() {
							return;
						},
					Err(_) => {
						let _ = sender.send(PipeEvent::Closed { failed: true });
						return;
					},
				}
			}
		})
		.map_err(|_| ())
}

fn join_readers(stdout: JoinHandle<()>, stderr: JoinHandle<()>) {
	let _ = stdout.join();
	let _ = stderr.join();
}

fn terminate_child(child: &mut Child) {
	if let Ok(process_group) = i32::try_from(child.id()) {
		// SAFETY: The child was started as its own process-group leader. A
		// negative PID targets that exact group and does not name another group.
		unsafe {
			libc::kill(-process_group, libc::SIGKILL);
		}
	}
	let _ = child.kill();
	let _ = child.wait();
}

fn append_output(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ()> {
	if output.len().saturating_add(bytes.len()) > MAX_LOGIN_OUTPUT_BYTES {
		return Err(());
	}
	output.extend_from_slice(bytes);
	Ok(())
}

fn parse_device_prompt(output: &[u8]) -> Option<Prompt> {
	let normalized = strip_sgr_sequences(output);
	contains_device_url(&normalized).then_some(())?;
	Some(Prompt {
		verification_url: DEVICE_LOGIN_URL,
		user_code: parse_device_code(&normalized)?,
	})
}

fn strip_sgr_sequences(output: &[u8]) -> Vec<u8> {
	let mut normalized = Vec::with_capacity(output.len());
	let mut offset = 0;
	while offset < output.len() {
		if output[offset..].starts_with(b"\x1b[") {
			let mut cursor = offset + 2;
			while cursor < output.len() && (0x30..=0x3f).contains(&output[cursor]) {
				cursor += 1;
			}
			while cursor < output.len() && (0x20..=0x2f).contains(&output[cursor]) {
				cursor += 1;
			}
			if output.get(cursor) == Some(&b'm') {
				offset = cursor + 1;
				continue;
			}
		}
		normalized.push(output[offset]);
		offset += 1;
	}
	normalized
}

fn contains_device_url(output: &[u8]) -> bool {
	output.windows(DEVICE_LOGIN_URL.len()).any(|window| window == DEVICE_LOGIN_URL.as_bytes())
}

fn parse_device_code(output: &[u8]) -> Option<String> {
	for suffix_length in [4_usize, 5] {
		let code_length = 5 + suffix_length;
		if output.len() < code_length {
			continue;
		}
		for start in 0..=output.len() - code_length {
			let end = start + code_length;
			let candidate = &output[start..end];
			if candidate[4] != b'-' {
				continue;
			}
			if !candidate[..4]
				.iter()
				.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
				|| !candidate[5..]
					.iter()
					.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
			{
				continue;
			}
			if start > 0 && is_device_code_token_byte(output[start - 1]) {
				continue;
			}
			if end < output.len() && is_device_code_token_byte(output[end]) {
				continue;
			}
			return String::from_utf8(candidate.to_vec()).ok();
		}
	}
	None
}

fn is_device_code_token_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'-'
}

struct LoginHome {
	path: PathBuf,
	device: u64,
	inode: u64,
	cleaned: bool,
}

impl LoginHome {
	fn create(session_id: &str) -> Result<Self, ()> {
		let base = fs::canonicalize(std::env::temp_dir()).map_err(|_| ())?;
		let path = base.join(format!("decodex-codex-device-login-{session_id}"));
		fs::create_dir(&path).map_err(|_| ())?;
		if fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).is_err() {
			let _ = fs::remove_dir(&path);
			return Err(());
		}
		let metadata = match fs::symlink_metadata(&path) {
			Ok(metadata) => metadata,
			Err(_) => {
				let _ = fs::remove_dir(&path);
				return Err(());
			},
		};
		if !metadata.file_type().is_dir()
			|| metadata.uid() != unsafe { libc::geteuid() }
			|| metadata.permissions().mode() & 0o077 != 0
		{
			let _ = fs::remove_dir(&path);
			return Err(());
		}
		Ok(Self { path, device: metadata.dev(), inode: metadata.ino(), cleaned: false })
	}

	fn path(&self) -> &Path {
		&self.path
	}

	fn cleanup(&mut self) -> Result<(), ()> {
		if self.cleaned {
			return Ok(());
		}
		let metadata = match fs::symlink_metadata(&self.path) {
			Ok(metadata) => metadata,
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
				self.cleaned = true;
				return Ok(());
			},
			Err(_) => return Err(()),
		};
		if !metadata.file_type().is_dir()
			|| metadata.dev() != self.device
			|| metadata.ino() != self.inode
		{
			return Err(());
		}
		fs::remove_dir_all(&self.path).map_err(|_| ())?;
		match fs::symlink_metadata(&self.path) {
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
				self.cleaned = true;
				Ok(())
			},
			Ok(_) | Err(_) => Err(()),
		}
	}
}

impl Drop for LoginHome {
	fn drop(&mut self) {
		let _ = self.cleanup();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const TTY_BUFFERING_FIXTURE: &str = "DECODEX_TTY_BUFFERING_FIXTURE";

	fn entity_id(value: &str) -> EntityId {
		EntityId::new(value).expect("fixture entity ID")
	}

	fn start(install_mode: InstallMode) -> Start {
		start_with_login_method(install_mode, LoginMethod::BrowserRedirect)
	}

	fn start_with_login_method(install_mode: InstallMode, login_method: LoginMethod) -> Start {
		Start {
			session_id: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned(),
			operation_id: entity_id("028f0f9e-7b6e-4a31-8f4c-1d2e3f405163"),
			account_id: entity_id("038f0f9e-7b6e-4a31-8f4c-1d2e3f405164"),
			idempotency_key: IdempotencyKey::new("account-login-fixture")
				.expect("fixture idempotency key"),
			codex_bin: PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
			login_method,
			install_mode,
		}
	}

	#[test]
	fn login_methods_select_exact_argv_and_the_private_file_credential_store() {
		let browser = login_command(
			Path::new("/Applications/ChatGPT.app/Contents/Resources/codex"),
			Path::new("/private/tmp/decodex-login"),
			LoginMethod::BrowserRedirect,
		);
		let browser_args =
			browser.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
		let device_code = login_command(
			Path::new("/Applications/ChatGPT.app/Contents/Resources/codex"),
			Path::new("/private/tmp/decodex-login"),
			LoginMethod::DeviceCode,
		);
		let device_code_args = device_code
			.get_args()
			.map(|arg| arg.to_string_lossy().into_owned())
			.collect::<Vec<_>>();

		assert_eq!(browser_args, ["login", "-c", r#"cli_auth_credentials_store="file""#]);
		assert_eq!(
			device_code_args,
			["login", "--device-auth", "-c", r#"cli_auth_credentials_store="file""#],
		);
	}

	#[test]
	fn login_methods_start_in_distinct_closed_states() {
		let session_id = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let browser = Shared::new(session_id.to_owned(), LoginMethod::BrowserRedirect).status();
		let device_code = Shared::new(session_id.to_owned(), LoginMethod::DeviceCode).status();

		assert_eq!(browser.state, State::OpeningBrowser);
		assert_eq!(browser.prompt, None);
		assert_eq!(device_code.state, State::RequestingCode);
		assert_eq!(device_code.prompt, None);
	}

	#[test]
	fn device_prompt_is_observable_before_a_tty_buffered_child_exits() {
		let mut fixture = Command::new(std::env::current_exe().expect("current test executable"));
		fixture
			.arg("tty_buffering_fixture_child")
			.arg("--nocapture")
			.env(TTY_BUFFERING_FIXTURE, "1")
			.stdin(Stdio::null())
			.stderr(Stdio::piped())
			.process_group(0);
		let spawned =
			spawn_login_process(fixture, LoginMethod::DeviceCode).expect("TTY login fixture");
		assert!(matches!(&spawned.stdout, LoginStdout::PseudoTerminal(_)));
		let SpawnedLoginProcess { mut child, stdout, stderr } = spawned;
		let (sender, receiver) = mpsc::channel();
		let stdout_reader = spawn_reader(stdout, sender.clone()).expect("fixture stdout reader");
		let stderr_reader = spawn_reader(stderr, sender).expect("fixture stderr reader");
		let deadline = Instant::now() + Duration::from_secs(2);
		let mut output = Vec::new();
		let mut observed_prompt = false;
		while Instant::now() < deadline {
			match receiver.recv_timeout(CHILD_POLL_INTERVAL) {
				Ok(PipeEvent::Bytes(bytes)) => {
					append_output(&mut output, &bytes).expect("bounded fixture output");
					if parse_device_prompt(&output).is_some() {
						observed_prompt = true;
						break;
					}
				},
				Ok(PipeEvent::Closed { .. }) | Err(RecvTimeoutError::Disconnected) => break,
				Err(RecvTimeoutError::Timeout) => {},
			}
		}
		assert!(matches!(child.try_wait(), Ok(None)), "fixture must still be running");
		terminate_child(&mut child);
		join_readers(stdout_reader, stderr_reader);

		assert!(observed_prompt, "TTY stdout must publish the device prompt before child exit");
	}

	#[test]
	fn browser_login_stdout_remains_an_ordinary_pipe() {
		let mut fixture = Command::new("/usr/bin/true");
		fixture.stdin(Stdio::null()).stderr(Stdio::piped()).process_group(0);

		let spawned =
			spawn_login_process(fixture, LoginMethod::BrowserRedirect).expect("browser fixture");

		assert!(matches!(&spawned.stdout, LoginStdout::Pipe(_)));
		let SpawnedLoginProcess { mut child, stdout, stderr } = spawned;
		drop((stdout, stderr));
		assert!(child.wait().expect("browser fixture exit").success());
	}

	#[test]
	fn pseudo_terminal_descriptors_are_owner_only_close_on_exec_and_signal_slave_drop() {
		let terminal = PseudoTerminal::open().expect("owner-controlled pseudo-terminal");
		for fd in [&terminal.master, &terminal.slave] {
			// SAFETY: Each `OwnedFd` remains open for this query.
			let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
			assert_ne!(flags, -1);
			assert_ne!(flags & libc::FD_CLOEXEC, 0);
		}
		let slave_metadata = fd_metadata(&terminal.slave).expect("PTY slave metadata");
		assert_eq!(slave_metadata.st_uid, unsafe { libc::geteuid() });
		assert_eq!(slave_metadata.st_mode & 0o077, 0);

		let PseudoTerminal { master, slave } = terminal;
		let mut reader = PseudoTerminalReader(File::from(master));
		drop(slave);

		assert_eq!(reader.read(&mut [0_u8; 1]).expect("PTY slave closure"), 0);
	}

	#[test]
	fn tty_buffering_fixture_child() {
		if std::env::var_os(TTY_BUFFERING_FIXTURE).is_none() {
			return;
		}
		let prompt = format!("Open {DEVICE_LOGIN_URL}\\nCode: ABCD-EFGH\\n");
		// SAFETY: `STDOUT_FILENO` is open for the child. The fixture deliberately
		// makes its pre-exit prompt conditional on the same terminal boundary that
		// selects line buffering in the official Codex child.
		if unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1 {
			// SAFETY: `prompt` supplies exactly `len` readable bytes for this write.
			let written = unsafe {
				libc::write(
					libc::STDOUT_FILENO,
					prompt.as_ptr().cast(),
					prompt.len(),
				)
			};
			assert_eq!(written, prompt.len() as isize);
		}
		thread::sleep(Duration::from_secs(30));
	}

	#[test]
	fn parser_accepts_current_and_historical_bounded_device_prompts() {
		let current =
			b"\x1b[32mOpen https://auth.openai.com/codex/device\x1b[0m\nCode: ABCD-EFGH\n";
		let historical = b"Open https://auth.openai.com/codex/device\nCode: AB12-CDE34\n";
		let terminal = b"Open https://auth.openai.com/codex/device\r\nCode: \x1b[94mABCD-12345\x1b[0m\r\n";

		assert_eq!(
			parse_device_prompt(current),
			Some(Prompt {
				verification_url: DEVICE_LOGIN_URL,
				user_code: "ABCD-EFGH".to_owned(),
			}),
		);
		assert_eq!(parse_device_code(historical).as_deref(), Some("AB12-CDE34"));
		assert_eq!(
			parse_device_prompt(terminal).map(|prompt| prompt.user_code),
			Some("ABCD-12345".to_owned()),
		);
	}

	#[test]
	fn parser_rejects_unbounded_or_noncanonical_device_prompts() {
		for output in [
			b"Open https://auth.openai.com/codex/device\nCode: abcd-efgh".as_slice(),
			b"Open https://auth.openai.com/codex/device\nCode: ABC-EFGH".as_slice(),
			b"Open https://auth.openai.com/codex/device\nCode: ABCD_EFGH".as_slice(),
			b"Open https://auth.openai.com/codex/device\nCode: XABCD-EFGHY".as_slice(),
			b"Open https://auth.openai.com/codex/device\nCode: ABCD-EFGHIJ".as_slice(),
			b"Open https://auth.openai.com/codex/device\nCode: X\x1b[1mABCD-EFGH\x1b[0m"
				.as_slice(),
			b"Open http://auth.openai.com/codex/device\nCode: ABCD-EFGH".as_slice(),
		] {
			assert_eq!(parse_device_prompt(output), None);
		}
	}

	#[test]
	fn enrollment_installs_with_the_typed_unrevisioned_command() {
		for login_method in [LoginMethod::BrowserRedirect, LoginMethod::DeviceCode] {
			let start =
				start_with_login_method(InstallMode::Enroll { enabled: true }, login_method);
			let source_descriptor =
				WireText::new("/private/tmp/device-login/auth.json").expect("fixture source");

			let (command, expected_revision) = install_command(&start, source_descriptor.clone());

			assert_eq!(expected_revision, None);
			assert_eq!(
				command,
				CommandPayload::EnrollAccountFromCredentialFile {
					operation_id: start.operation_id,
					account_id: start.account_id,
					enabled: true,
					source_descriptor,
				}
			);
		}
	}

	#[test]
	fn reauthentication_retains_its_revision_and_recovery_fences() {
		let recovery_operation_id = entity_id("048f0f9e-7b6e-4a31-8f4c-1d2e3f405165");
		let start = start(InstallMode::Reauthenticate {
			expected_revision: EntityRevision(7),
			recovery_operation_id: Some(recovery_operation_id.clone()),
		});
		let source_descriptor =
			WireText::new("/private/tmp/device-login/auth.json").expect("fixture source");

		let (command, expected_revision) = install_command(&start, source_descriptor.clone());

		assert_eq!(expected_revision, Some(EntityRevision(7)));
		assert_eq!(
			command,
			CommandPayload::ReauthenticateAccountFromCredentialFile {
				operation_id: start.operation_id,
				account_id: start.account_id,
				recovery_operation_id: Some(recovery_operation_id),
				source_descriptor,
			}
		);
	}

	#[test]
	fn duplicate_provider_is_a_closed_enrollment_failure() {
		let error = CommandError::AccountCommandRejected {
			rejection: AccountCommandRejectionDto::ProviderAlreadyEnrolled,
			actual_revision: None,
		};

		assert_eq!(map_command_error(&error), Failure::ProviderAlreadyEnrolled);
		assert_eq!(
			serde_json::to_value(Status::failed(
				"058f0f9e-7b6e-4a31-8f4c-1d2e3f405166".to_owned(),
				Failure::ProviderAlreadyEnrolled,
			))
			.expect("status must encode"),
			serde_json::json!({
				"session_id": "058f0f9e-7b6e-4a31-8f4c-1d2e3f405166",
				"state": "failed",
				"failure": "provider_already_enrolled",
			}),
		);
	}

	#[test]
	fn login_home_is_private_and_removed_on_drop() {
		let session_id = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let metadata = fs::metadata(&path).expect("login home metadata");

		assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
		drop(home);
		assert!(!path.exists());
	}

	#[test]
	fn login_home_cleanup_proves_absence() {
		let session_id = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		fs::write(path.join("auth.json"), b"secret").expect("temporary credential");

		home.cleanup().expect("explicit cleanup");

		assert!(!path.exists());
		assert!(home.cleaned);
	}

	#[test]
	fn login_home_cleanup_rejects_identity_drift() {
		let session_id = "038f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let moved = path.with_extension("moved");
		fs::rename(&path, &moved).expect("move original home");
		fs::create_dir(&path).expect("replacement home");

		assert!(home.cleanup().is_err());
		assert!(path.exists());

		fs::remove_dir(&path).expect("remove replacement");
		fs::rename(&moved, &path).expect("restore original");
		home.cleanup().expect("cleanup restored home");
	}

	#[test]
	fn completed_install_with_unproven_cleanup_is_outcome_unknown() {
		let session_id = "048f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let moved = path.with_extension("moved");
		fs::rename(&path, &moved).expect("move original home");
		fs::create_dir(&path).expect("replacement home");

		let status = finalize_login_status(
			&mut home,
			session_id.to_owned(),
			Status::completed(session_id.to_owned()),
		);

		assert_eq!(status.state, State::Failed);
		assert_eq!(status.failure, Some(Failure::OutcomeUnknown));

		fs::remove_dir(&path).expect("remove replacement");
		fs::rename(&moved, &path).expect("restore original");
		home.cleanup().expect("cleanup restored home");
	}

	#[test]
	fn uncertain_install_with_unproven_cleanup_remains_outcome_unknown() {
		let session_id = "048f0f9e-7b6e-4a31-8f4c-1d2e3f405163";
		let mut home = LoginHome::create(session_id).expect("private login home");
		let path = home.path().to_owned();
		let moved = path.with_extension("moved");
		fs::rename(&path, &moved).expect("move original home");
		fs::create_dir(&path).expect("replacement home");

		let status = finalize_login_status(
			&mut home,
			session_id.to_owned(),
			Status::failed(session_id.to_owned(), Failure::OutcomeUnknown),
		);

		assert_eq!(status.state, State::Failed);
		assert_eq!(status.failure, Some(Failure::OutcomeUnknown));

		fs::remove_dir(&path).expect("remove replacement");
		fs::rename(&moved, &path).expect("restore original");
		home.cleanup().expect("cleanup restored home");
	}

	#[test]
	fn successful_leader_exit_reaps_a_descendant_that_holds_login_pipes() {
		let session_id = "058f0f9e-7b6e-4a31-8f4c-1d2e3f405160".to_owned();
		let (manager, shared, _fixture) = pipe_holding_login_manager(&session_id);
		let started = Instant::now();
		while !shared.status().is_terminal() && started.elapsed() < Duration::from_secs(2) {
			thread::sleep(Duration::from_millis(5));
		}

		assert_eq!(shared.status().state, State::Completed);
		assert!(started.elapsed() < Duration::from_secs(2));
		drop(manager);
	}

	#[test]
	fn cancel_after_successful_leader_exit_preserves_completed_status() {
		let session_id = "058f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let (manager, _, _fixture) = pipe_holding_login_manager(&session_id);

		let started = Instant::now();
		let status = manager.cancel(&session_id);

		assert_eq!(status.state, State::Completed);
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[test]
	fn shutdown_after_successful_leader_exit_preserves_completed_status() {
		let session_id = "068f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let (manager, shared, _fixture) = pipe_holding_login_manager(&session_id);

		let started = Instant::now();
		manager.shutdown();

		assert_eq!(shared.status().state, State::Completed);
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[test]
	fn cancel_waits_for_worker_terminal_cleanup() {
		let manager = Manager::default();
		let session_id = "078f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let shared =
			Arc::new(Shared::new(session_id.clone(), LoginMethod::BrowserRedirect));
		let worker_shared = Arc::clone(&shared);
		let worker_session_id = session_id.clone();
		let worker = thread::spawn(move || {
			while !worker_shared.cancel_requested.load(Ordering::Acquire) {
				thread::yield_now();
			}
			worker_shared.set_status(Status::cancelled(worker_session_id));
		});
		*manager.lock_session() =
			Some(Session { session_id: session_id.clone(), shared, worker: Some(worker) });

		let status = manager.cancel(&session_id);

		assert_eq!(status.state, State::Cancelled);
		assert!(manager.lock_session().as_ref().is_some_and(|session| session.worker.is_none()));
	}

	#[test]
	fn shutdown_closes_start_fence_and_joins_installing_worker() {
		let manager = Manager::default();
		let session_id = "088f0f9e-7b6e-4a31-8f4c-1d2e3f405162".to_owned();
		let shared =
			Arc::new(Shared::new(session_id.clone(), LoginMethod::BrowserRedirect));
		shared.set_status(Status::installing(session_id.clone()));
		let worker_shared = Arc::clone(&shared);
		let worker_session_id = session_id.clone();
		let worker = thread::spawn(move || {
			thread::sleep(Duration::from_millis(5));
			worker_shared.set_status(Status::completed(worker_session_id));
		});
		*manager.lock_session() = Some(Session {
			session_id: session_id.clone(),
			shared: Arc::clone(&shared),
			worker: Some(worker),
		});

		manager.shutdown();

		assert!(manager.closed.load(Ordering::Acquire));
		assert_eq!(shared.status().state, State::Completed);
		assert!(manager.lock_session().as_ref().is_some_and(|session| session.worker.is_none()));
	}

	fn pipe_holding_login_manager(session_id: &str) -> (Manager, Arc<Shared>, tempfile::TempDir) {
		let manager = Manager::default();
		let shared =
			Arc::new(Shared::new(session_id.to_owned(), LoginMethod::BrowserRedirect));
		let worker_shared = Arc::clone(&shared);
		let worker_session_id = session_id.to_owned();
		let fixture = tempfile::tempdir().expect("fixture directory");
		let leader_path = fixture.path().join("leader.pid");
		let worker_leader_path = leader_path.clone();
		let worker = thread::spawn(move || {
			let mut child = Command::new("/bin/sh")
				.arg("-c")
				.arg(
					"printf '%s' \"$$\" > \"$1\"; \
					 printf 'Waiting for browser login\\n'; \
					 (sleep 60) & exit 0",
				)
				.arg("decodex-login-fixture")
				.arg(worker_leader_path)
				.stdin(Stdio::null())
				.stdout(Stdio::piped())
				.stderr(Stdio::piped())
				.process_group(0)
				.spawn()
				.expect("login fixture");
			let stdout = child.stdout.take().expect("fixture stdout");
			let stderr = child.stderr.take().expect("fixture stderr");
			let (sender, receiver) = mpsc::channel();
			let stdout_reader =
				spawn_reader(stdout, sender.clone()).expect("fixture stdout reader");
			let stderr_reader = spawn_reader(stderr, sender).expect("fixture stderr reader");
			let status = match collect_login_child(
				&worker_shared,
				&mut child,
				receiver,
				stdout_reader,
					stderr_reader,
					&worker_session_id,
					LoginMethod::BrowserRedirect,
				) {
				Ok(output) if output.exit.success() && !output.reader_failed =>
					Status::completed(worker_session_id),
				Ok(_) => Status::failed(worker_session_id, Failure::LoginFailed),
				Err(status) => status,
			};
			worker_shared.set_status(status);
		});
		*manager.lock_session() = Some(Session {
			session_id: session_id.to_owned(),
			shared: Arc::clone(&shared),
			worker: Some(worker),
		});

		let mut leader_reaped = false;
		for _ in 0..200 {
			if let Ok(text) = fs::read_to_string(&leader_path)
				&& let Ok(pid) = text.parse::<i32>()
			{
				// SAFETY: Signal zero performs a liveness query and does not
				// mutate the fixture process.
				let result = unsafe { libc::kill(pid, 0) };
				if result == -1
					&& std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
				{
					leader_reaped = true;
					break;
				}
			}
			thread::sleep(Duration::from_millis(5));
		}
		assert!(leader_reaped, "fixture leader must exit while its descendant holds the pipes");

		(manager, shared, fixture)
	}
}
