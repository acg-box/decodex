use std::{
	fs,
	io::Read,
	os::unix::{
		fs::{MetadataExt as _, PermissionsExt as _},
		process::CommandExt as _,
	},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
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
use serde::Serialize;
use tokio::runtime::Handle;

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
	WaitingForBrowser,
	Installing,
	Completed,
	Failed,
	Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Status {
	session_id: String,
	state: State,
	#[serde(skip_serializing_if = "Option::is_none")]
	failure: Option<Failure>,
}

impl Status {
	fn opening_browser(session_id: String) -> Self {
		Self { session_id, state: State::OpeningBrowser, failure: None }
	}

	fn waiting(session_id: String) -> Self {
		Self { session_id, state: State::WaitingForBrowser, failure: None }
	}

	fn installing(session_id: String) -> Self {
		Self { session_id, state: State::Installing, failure: None }
	}

	fn completed(session_id: String) -> Self {
		Self { session_id, state: State::Completed, failure: None }
	}

	fn failed(session_id: String, failure: Failure) -> Self {
		Self { session_id, state: State::Failed, failure: Some(failure) }
	}

	fn cancelled(session_id: String) -> Self {
		Self { session_id, state: State::Cancelled, failure: None }
	}

	fn is_terminal(&self) -> bool {
		matches!(self.state, State::Completed | State::Failed | State::Cancelled)
	}
}

pub(crate) struct Start {
	pub session_id: String,
	pub operation_id: EntityId,
	pub account_id: EntityId,
	pub expected_revision: EntityRevision,
	pub recovery_operation_id: Option<EntityId>,
	pub idempotency_key: IdempotencyKey,
	pub codex_bin: PathBuf,
}

struct Shared {
	status: Mutex<Status>,
	cancel_requested: AtomicBool,
}

impl Shared {
	fn new(session_id: String) -> Self {
		Self {
			status: Mutex::new(Status::opening_browser(session_id)),
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
		let shared = Arc::new(Shared::new(start.session_id.clone()));
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
	let mut child = match browser_login_command(&codex_bin, login_home).spawn() {
		Ok(child) => child,
		Err(_) => return Status::failed(session_id, Failure::CodexUnavailable),
	};
	shared.set_status(Status::waiting(session_id.clone()));
	let Some(stdout) = child.stdout.take() else {
		terminate_child(&mut child);
		return Status::failed(session_id, Failure::LoginFailed);
	};
	let Some(stderr) = child.stderr.take() else {
		terminate_child(&mut child);
		return Status::failed(session_id, Failure::LoginFailed);
	};
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
	) {
		Ok(output) => output,
		Err(status) => return status,
	};
	if output.reader_failed || !output.exit.success() {
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

fn browser_login_command(codex_bin: &Path, login_home: &Path) -> Command {
	let mut command = Command::new(codex_bin);
	command
		.arg("login")
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
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.process_group(0);
	command
}

struct LoginChildOutput {
	exit: ExitStatus,
	reader_failed: bool,
}

fn collect_login_child(
	shared: &Shared,
	child: &mut Child,
	receiver: Receiver<PipeEvent>,
	stdout_reader: JoinHandle<()>,
	stderr_reader: JoinHandle<()>,
	session_id: &str,
) -> Result<LoginChildOutput, Status> {
	let deadline = Instant::now() + LOGIN_TIMEOUT;
	let mut output = Vec::new();
	let mut reader_failed = false;
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
			Ok(PipeEvent::Bytes(bytes)) =>
				if append_output(&mut output, &bytes).is_err() {
					terminate_child(child);
					join_readers(stdout_reader, stderr_reader);
					return Err(Status::failed(session_id.to_owned(), Failure::LoginFailed));
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
					PipeEvent::Bytes(bytes) =>
						if append_output(&mut output, &bytes).is_err() {
							return Err(Status::failed(
								session_id.to_owned(),
								Failure::LoginFailed,
							));
						},
					PipeEvent::Closed { failed } => reader_failed |= failed,
				}
			}
			return Ok(LoginChildOutput { exit, reader_failed });
		}
	}
}

async fn install_credential(
	profile: ClientProfile,
	start: Start,
	auth_path: PathBuf,
) -> Result<(), Failure> {
	let source_descriptor = WireText::new(auth_path.to_string_lossy().into_owned())
		.map_err(|_| Failure::LoginFailed)?;
	for attempt in 0..INSTALL_DISPATCH_ATTEMPTS {
		let response = AccountClient::new(profile.clone())
			.execute(
				CommandPayload::ReauthenticateAccountFromCredentialFile {
					operation_id: start.operation_id.clone(),
					account_id: start.account_id.clone(),
					recovery_operation_id: start.recovery_operation_id.clone(),
					source_descriptor: source_descriptor.clone(),
				},
				Some(start.expected_revision),
				start.idempotency_key.clone(),
			)
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

fn map_client_failure(_failure: ClientFailure) -> Failure {
	Failure::ServiceUnavailable
}

fn map_command_error(error: &CommandError) -> Failure {
	match error {
		CommandError::ExpectedRevisionMismatch { .. } => Failure::AccountChanged,
		CommandError::AccountCommandRejected { rejection, .. } => match rejection {
			AccountCommandRejectionDto::ProviderMismatch => Failure::AccountMismatch,
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

	#[test]
	fn browser_login_forces_the_private_file_credential_store() {
		let command = browser_login_command(
			Path::new("/Applications/ChatGPT.app/Contents/Resources/codex"),
			Path::new("/private/tmp/decodex-login"),
		);
		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();

		assert_eq!(args, ["login", "-c", r#"cli_auth_credentials_store="file""#]);
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
		let shared = Arc::new(Shared::new(session_id.clone()));
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
		let shared = Arc::new(Shared::new(session_id.clone()));
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
		let shared = Arc::new(Shared::new(session_id.to_owned()));
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
