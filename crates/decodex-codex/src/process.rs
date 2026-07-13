#[cfg(unix)] use std::os::unix::process::CommandExt as _;
use std::{
	collections::BTreeMap,
	env,
	ffi::{OsStr, OsString},
	fmt::{Debug, Display, Formatter},
	fs,
	io::{self, BufRead, BufReader, Write as _},
	path::PathBuf,
	process::{Child, ChildStdin, Command, ExitStatus, Stdio},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
	},
	thread,
	time::{Duration, Instant},
};

use libc::{EPERM, ESRCH, RLIMIT_FSIZE, SIGKILL, SIGTERM};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{self, Value};
use sha2::{Digest as _, Sha256};
use tempfile::{NamedTempFile, TempDir};

use crate::{
	BuildId, Capability, CapabilityCache, CapabilityProfile, LiveMethodOutcome, MethodObservation,
	SchemaContract, SchemaMarker, ThreadSummary,
	protocol::{
		AccountReadResponse, ClientInfo, InitializeCapabilities, InitializeParams,
		InitializeResponse, JsonRpcResponse, ThreadListParams, ThreadListResponse,
	},
	schema::{self, GeneratedSchemaEvidence},
};

const CREDENTIAL_ENVIRONMENT: &[&str] = &[
	"CODEX_HOME",
	"OPENAI_API_KEY",
	"CODEX_API_KEY",
	"CHATGPT_ACCESS_TOKEN",
	"AWS_ACCESS_KEY_ID",
	"AWS_SECRET_ACCESS_KEY",
	"AWS_SESSION_TOKEN",
];
const MAX_PROTOCOL_FRAME_BYTES: usize = 1_024 * 1_024;
const PROTOCOL_QUEUE_CAPACITY: usize = 64;
const THREAD_LIST_LIMIT: usize = 100;
const MAX_VERSION_OUTPUT_BYTES: u64 = 4 * 1_024;
const MAX_PREFLIGHT_FILE_BYTES: u64 = 16 * 1_024 * 1_024;

/// Immutable shared-home account authority. There is intentionally no rebinding API.
#[derive(Clone, Debug)]
pub struct AccountBinding {
	expected_codex_home: PathBuf,
}
impl AccountBinding {
	/// Bind one child to the account resolved from the child's immutable Codex home.
	pub fn shared_home() -> Result<Self, SupervisionError> {
		let home = env::var_os("HOME")
			.filter(|home| !home.is_empty())
			.ok_or(SupervisionError::InvalidBinding)?;

		Ok(Self { expected_codex_home: PathBuf::from(home).join(".codex") })
	}

	#[cfg(test)]
	fn for_test(expected_codex_home: PathBuf) -> Self {
		Self { expected_codex_home }
	}
}

/// Exact executable contract for one supervised app-server build.
#[derive(Clone, Debug)]
pub struct AppServerCommand {
	program: PathBuf,
	app_server_args: Vec<OsString>,
	version_args: Vec<OsString>,
	schema_args: Vec<OsString>,
	working_directory: PathBuf,
}
impl AppServerCommand {
	/// Construct the only production command shape: Codex app-server plus read-only attestation.
	///
	/// Launch program and arguments are fixed. Tests use a private fixture constructor.
	///
	/// ```compile_fail
	/// use decodex_codex::AppServerCommand;
	///
	/// let _ = AppServerCommand::new("python3", ".");
	/// ```
	pub fn new(working_directory: impl Into<PathBuf>) -> Self {
		Self {
			program: "codex".into(),
			app_server_args: vec!["app-server".into(), "--stdio".into()],
			version_args: vec!["--version".into()],
			schema_args: vec![
				"app-server".into(),
				"generate-json-schema".into(),
				"--experimental".into(),
				"--out".into(),
			],
			working_directory: working_directory.into(),
		}
	}

	#[cfg(test)]
	fn new_for_test(
		program: impl Into<PathBuf>,
		app_server_args: impl IntoIterator<Item = impl Into<OsString>>,
		version_args: impl IntoIterator<Item = impl Into<OsString>>,
		schema_args: impl IntoIterator<Item = impl Into<OsString>>,
		working_directory: impl Into<PathBuf>,
	) -> Self {
		Self {
			program: program.into(),
			app_server_args: app_server_args.into_iter().map(Into::into).collect(),
			version_args: version_args.into_iter().map(Into::into).collect(),
			schema_args: schema_args.into_iter().map(Into::into).collect(),
			working_directory: working_directory.into(),
		}
	}
}

/// Pseudonymous receipt for the account reported by `account/read`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountIdentity(String);
impl AccountIdentity {
	/// Return the pseudonymous active-account receipt.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Owned app-server child and its immutable account authority.
pub struct SupervisedProcess {
	child: Child,
	stdin: ChildStdin,
	stdout: Receiver<Vec<u8>>,
	protocol_limit_exceeded: Arc<AtomicBool>,
	binding: AccountBinding,
	command: AppServerCommand,
	next_request_id: u64,
	stopped: bool,
}
impl SupervisedProcess {
	fn spawn(command: AppServerCommand, binding: AccountBinding) -> Result<Self, SupervisionError> {
		let mut process = Command::new(&command.program);

		process
			.args(&command.app_server_args)
			.current_dir(&command.working_directory)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null());

		for name in CREDENTIAL_ENVIRONMENT {
			process.env_remove(name);
		}

		configure_process_group(&mut process, None);

		let mut child = process.spawn().map_err(|_| SupervisionError::SpawnFailed)?;
		let stdin = child.stdin.take().ok_or(SupervisionError::StdinUnavailable)?;
		let stdout = child.stdout.take().ok_or(SupervisionError::InvalidProtocol)?;
		let (sender, receiver) = mpsc::sync_channel(PROTOCOL_QUEUE_CAPACITY);
		let protocol_limit_exceeded = Arc::new(AtomicBool::new(false));
		let reader_limit_exceeded = Arc::clone(&protocol_limit_exceeded);

		thread::spawn(move || {
			pump_stdout(BufReader::new(stdout), sender, reader_limit_exceeded);
		});

		Ok(Self {
			child,
			stdin,
			stdout: receiver,
			protocol_limit_exceeded,
			binding,
			command,
			next_request_id: 1,
			stopped: false,
		})
	}

	/// OS process identifier, used only for bounded supervision evidence.
	pub fn process_id(&self) -> u32 {
		self.child.id()
	}

	/// Stop this child and its process group within a bounded deadline.
	pub fn shutdown(mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		self.shutdown_inner(timeout)
	}

	/// Restart using the same immutable account binding and exact command/build.
	pub fn restart(mut self, timeout: Duration) -> Result<Self, SupervisionError> {
		self.shutdown_inner(timeout)?;

		Self::spawn(self.command.clone(), self.binding.clone())
	}

	fn request<P, R>(
		&mut self,
		method: ReadOnlyMethod,
		params: &P,
		timeout: Duration,
	) -> Result<R, ProbeError>
	where
		P: Serialize,
		R: DeserializeOwned,
	{
		let request_id = self.next_request_id;

		self.next_request_id += 1;

		self.write_json(&serde_json::json!({
			"jsonrpc": "2.0",
			"id": request_id,
			"method": method.as_str(),
			"params": params,
		}))?;

		let deadline = Instant::now() + timeout;

		loop {
			let remaining = deadline.saturating_duration_since(Instant::now());

			if remaining.is_zero() {
				return Err(ProbeError::Supervision(SupervisionError::ResponseTimeout));
			}

			let line = match self.stdout.recv_timeout(remaining) {
				Ok(line) => line,
				Err(RecvTimeoutError::Timeout) => {
					return Err(ProbeError::Supervision(SupervisionError::ResponseTimeout));
				},
				Err(RecvTimeoutError::Disconnected) => {
					let error = if self.protocol_limit_exceeded.load(Ordering::Acquire) {
						SupervisionError::ProtocolLimitExceeded
					} else {
						SupervisionError::ProcessExited
					};

					return Err(ProbeError::Supervision(error));
				},
			};
			let value: Value = serde_json::from_slice(&line)
				.map_err(|_| ProbeError::Supervision(SupervisionError::InvalidProtocol))?;

			if value.get("id").and_then(Value::as_u64) == Some(request_id) {
				let response: JsonRpcResponse<R> = serde_json::from_value(value)
					.map_err(|_| ProbeError::Supervision(SupervisionError::InvalidProtocol))?;

				if response.id != request_id {
					return Err(ProbeError::Supervision(SupervisionError::InvalidProtocol));
				}

				if let Some(error) = response.error {
					return Err(ProbeError::MethodRejected { method, code: error.code });
				}

				return response
					.result
					.ok_or(ProbeError::Supervision(SupervisionError::InvalidProtocol));
			}
			if value.get("method").is_some() && value.get("id").is_some() {
				let id = value.get("id").cloned().unwrap_or(Value::Null);

				self.write_json(&serde_json::json!({
					"jsonrpc": "2.0",
					"id": id,
					"error": {"code": -32_601, "message": "read-only probe does not service requests"}
				}))?;
			}
		}
	}

	fn notify<P>(&mut self, method: &str, params: &P) -> Result<(), ProbeError>
	where
		P: Serialize,
	{
		self.write_json(&serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params}))
	}

	fn write_json(&mut self, value: &Value) -> Result<(), ProbeError> {
		serde_json::to_writer(&mut self.stdin, value)
			.map_err(|_| ProbeError::Supervision(SupervisionError::WriteFailed))?;

		self.stdin
			.write_all(b"\n")
			.and_then(|_| self.stdin.flush())
			.map_err(|_| ProbeError::Supervision(SupervisionError::WriteFailed))
	}

	fn shutdown_inner(&mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		if self.stopped {
			return Ok(ShutdownOutcome::Exited);
		}

		let pid = self.child.id();
		let leader_exited =
			self.child.try_wait().map_err(|_| SupervisionError::ShutdownFailed)?.is_some();

		if !process_group_exists(pid)? {
			self.stopped = true;

			return Ok(ShutdownOutcome::Exited);
		}

		signal_process_group(pid, SIGTERM)?;

		let started = Instant::now();
		let term_deadline = started + timeout / 2;
		let hard_deadline = started + timeout;

		while Instant::now() < term_deadline {
			let _ = self.child.try_wait().map_err(|_| SupervisionError::ShutdownFailed)?;

			if !process_group_exists(pid)? {
				self.stopped = true;

				return Ok(if leader_exited {
					ShutdownOutcome::Exited
				} else {
					ShutdownOutcome::Terminated
				});
			}

			thread::sleep(Duration::from_millis(10));
		}

		signal_process_group(pid, SIGKILL)?;

		while Instant::now() < hard_deadline {
			let _ = self.child.try_wait().map_err(|_| SupervisionError::ShutdownFailed)?;

			if !process_group_exists(pid)? {
				self.stopped = true;

				return Ok(ShutdownOutcome::KilledAfterTimeout);
			}

			thread::sleep(Duration::from_millis(10));
		}

		Err(SupervisionError::ShutdownFailed)
	}
}

impl Debug for SupervisedProcess {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("SupervisedProcess")
			.field("pid", &self.child.id())
			.field("binding", &self.binding)
			.finish()
	}
}

impl Drop for SupervisedProcess {
	fn drop(&mut self) {
		let _ = self.shutdown_inner(Duration::from_millis(250));
	}
}

/// Typed result from `initialize` plus a bounded `thread/list`; no raw JSON escapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadOnlyProbeResult {
	/// Exact-build negotiated capability profile.
	pub profile: CapabilityProfile,
	/// Redacted bounded thread summaries.
	pub threads: Vec<ThreadSummary>,
	/// Whether Codex returned a continuation cursor.
	pub list_is_paginated: bool,
	/// Opaque active-account receipt, derived only after `account/read`.
	pub account_identity: Option<AccountIdentity>,
}

/// Fake/live probe that cannot construct a turn or account-selection request.
pub struct ReadOnlyProbe {
	command: AppServerCommand,
	binding: AccountBinding,
	schema_marker: SchemaMarker,
	enforce_generated_digests: bool,
	timeout: Duration,
}
impl ReadOnlyProbe {
	/// Configure a probe. Schema validation is deferred to `run` but precedes spawn.
	pub fn new(command: AppServerCommand, binding: AccountBinding, timeout: Duration) -> Self {
		Self {
			command,
			binding,
			schema_marker: SchemaMarker::accepted(),
			enforce_generated_digests: true,
			timeout,
		}
	}

	#[cfg(test)]
	fn new_for_test(
		command: AppServerCommand,
		binding: AccountBinding,
		schema_marker: SchemaMarker,
		timeout: Duration,
	) -> Self {
		Self { command, binding, schema_marker, enforce_generated_digests: false, timeout }
	}

	/// Attest the executable, then run initialize, account/read, and thread/list.
	pub fn run(self, cache: &mut CapabilityCache) -> Result<ReadOnlyProbeResult, ProbeError> {
		let schema = SchemaContract::validate(self.schema_marker.clone())
			.map_err(|markers| ProbeError::SchemaMissing { markers })?;
		let expected_digests =
			self.enforce_generated_digests.then_some(self.schema_marker.canonical_digests());
		let (build, generated) = attest_executable(&self.command, expected_digests, self.timeout)?;
		let _ = schema;
		let mut process = SupervisedProcess::spawn(self.command, self.binding)?;
		let initialize = process.request::<_, InitializeResponse>(
			ReadOnlyMethod::Initialize,
			&InitializeParams {
				client_info: ClientInfo { name: "decodex", version: env!("CARGO_PKG_VERSION") },
				capabilities: InitializeCapabilities { experimental_api: true },
			},
			self.timeout,
		)?;

		validate_initialize(&initialize, &process.binding)?;

		process.notify("initialized", &serde_json::json!({}))?;

		let account = process.request::<_, AccountReadResponse>(
			ReadOnlyMethod::AccountRead,
			&serde_json::json!({}),
			self.timeout,
		)?;
		let requires_openai_auth = account.requires_openai_auth;
		let account = account.account.ok_or(SupervisionError::AccountUnavailable)?;
		let account_identity = {
			let mut digest = Sha256::new();

			digest.update(account.kind.as_bytes());
			digest.update([0]);

			if let Some(email) = account.email {
				digest.update(email.as_bytes());
			}

			digest.update([u8::from(requires_openai_auth)]);

			AccountIdentity(format!("sha256:{}", schema::hex_digest(digest.finalize().as_ref())))
		};
		let list = process.request::<_, ThreadListResponse>(
			ReadOnlyMethod::ThreadList,
			&ThreadListParams { limit: THREAD_LIST_LIMIT as u32 },
			self.timeout,
		)?;

		if list.data.len() > THREAD_LIST_LIMIT {
			return Err(SupervisionError::ProtocolLimitExceeded.into());
		}

		let list_is_paginated = list.next_cursor.is_some();
		let profile = CapabilityProfile::negotiate(
			build,
			generated.fingerprint.clone(),
			generated.contract(),
			[
				MethodObservation {
					capability: Capability::Initialize,
					outcome: LiveMethodOutcome::Supported,
				},
				MethodObservation {
					capability: Capability::ThreadList,
					outcome: LiveMethodOutcome::Supported,
				},
			],
		);

		process.shutdown(Duration::from_secs(1))?;
		cache.insert(profile.clone()).map_err(|_| ProbeError::CapabilityConflict)?;

		Ok(ReadOnlyProbeResult {
			profile,
			threads: list.data.into_iter().map(ThreadSummary::from).collect(),
			list_is_paginated,
			account_identity: Some(account_identity),
		})
	}
}

/// Bounded child shutdown result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownOutcome {
	/// Child had already exited.
	Exited,
	/// Child process group exited after termination.
	Terminated,
	/// Child process group required kill after the deadline.
	KilledAfterTimeout,
}

/// Sanitized process-supervision failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SupervisionError {
	/// The shared-home binding could not be established.
	InvalidBinding,
	/// Initialize reported a different Codex home than the immutable binding.
	CodexHomeMismatch,
	/// No active account identity was available after initialization.
	AccountUnavailable,
	/// Executable version/schema attestation failed.
	PreflightFailed,
	/// Child could not be spawned.
	SpawnFailed,
	/// Child stdin was not available.
	StdinUnavailable,
	/// A request could not be written.
	WriteFailed,
	/// No response arrived within the bounded timeout.
	ResponseTimeout,
	/// Child exited before producing a response.
	ProcessExited,
	/// Child emitted a response outside the typed contract.
	InvalidProtocol,
	/// Child exceeded a bounded protocol frame, queue, or result limit.
	ProtocolLimitExceeded,
	/// Process-group shutdown could not be completed.
	ShutdownFailed,
}
impl std::error::Error for SupervisionError {}

impl Display for SupervisionError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

/// Sanitized read-only capability-probe failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeError {
	/// Required schema markers were absent before spawn.
	SchemaMissing {
		/// Exact missing marker names.
		markers: Vec<String>,
	},
	/// Process lifecycle or transport failed.
	Supervision(SupervisionError),
	/// A read-only method returned a typed JSON-RPC rejection.
	MethodRejected {
		/// Closed read-only method classification.
		method: ReadOnlyMethod,
		/// Numeric JSON-RPC code; raw messages are discarded.
		code: i64,
	},
	/// A different profile already occupied this exact-build cache key.
	CapabilityConflict,
}
impl std::error::Error for ProbeError {}

impl Display for ProbeError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

impl From<SupervisionError> for ProbeError {
	fn from(value: SupervisionError) -> Self {
		Self::Supervision(value)
	}
}

/// Read-only methods available to the bounded foundation probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadOnlyMethod {
	/// Initialize the app-server connection.
	Initialize,
	/// Read the immutable process account.
	AccountRead,
	/// Read a bounded page of threads.
	ThreadList,
}
impl ReadOnlyMethod {
	fn as_str(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountRead => "account/read",
			Self::ThreadList => "thread/list",
		}
	}
}

fn read_bounded_line(reader: &mut impl BufRead, limit: usize) -> Result<Option<Vec<u8>>, ()> {
	let mut line = Vec::new();

	loop {
		let buffer = reader.fill_buf().map_err(|_| ())?;

		if buffer.is_empty() {
			return if line.is_empty() { Ok(None) } else { Ok(Some(line)) };
		}

		let consumed =
			buffer.iter().position(|byte| *byte == b'\n').map_or(buffer.len(), |at| at + 1);

		if line.len().saturating_add(consumed) > limit {
			return Err(());
		}

		line.extend_from_slice(&buffer[..consumed]);
		reader.consume(consumed);

		if line.last() == Some(&b'\n') {
			return Ok(Some(line));
		}
	}
}

fn pump_stdout(
	mut reader: impl BufRead,
	sender: SyncSender<Vec<u8>>,
	protocol_limit_exceeded: Arc<AtomicBool>,
) {
	loop {
		match read_bounded_line(&mut reader, MAX_PROTOCOL_FRAME_BYTES) {
			Ok(Some(line)) => match sender.try_send(line) {
				Ok(()) => {},
				Err(TrySendError::Full(_)) => {
					protocol_limit_exceeded.store(true, Ordering::Release);

					break;
				},
				Err(TrySendError::Disconnected(_)) => break,
			},
			Ok(None) => break,
			Err(()) => {
				protocol_limit_exceeded.store(true, Ordering::Release);

				break;
			},
		}
	}
}

fn attest_executable(
	command: &AppServerCommand,
	expected_digests: Option<&BTreeMap<String, String>>,
	timeout: Duration,
) -> Result<(BuildId, GeneratedSchemaEvidence), ProbeError> {
	let deadline = Instant::now() + timeout;
	let version_output = NamedTempFile::new().map_err(|_| SupervisionError::PreflightFailed)?;
	let version_writer = version_output.reopen().map_err(|_| SupervisionError::PreflightFailed)?;
	let version_status = run_preflight_command(
		command,
		&command.version_args,
		Stdio::from(version_writer),
		preflight_remaining(deadline)?,
		MAX_VERSION_OUTPUT_BYTES,
	)?;

	if !version_status.success() {
		return Err(SupervisionError::PreflightFailed.into());
	}
	if version_output.as_file().metadata().map_err(|_| SupervisionError::PreflightFailed)?.len()
		> MAX_VERSION_OUTPUT_BYTES
	{
		return Err(SupervisionError::PreflightFailed.into());
	}

	let version = fs::read(version_output.path()).map_err(|_| SupervisionError::PreflightFailed)?;
	let version = String::from_utf8(version).map_err(|_| SupervisionError::PreflightFailed)?;
	let build = BuildId::new(version.trim()).map_err(|_| SupervisionError::PreflightFailed)?;
	let schema_directory = TempDir::new().map_err(|_| SupervisionError::PreflightFailed)?;
	let mut schema_args = command.schema_args.clone();

	schema_args.push(schema_directory.path().as_os_str().to_owned());

	let status = run_preflight_command(
		command,
		&schema_args,
		Stdio::null(),
		preflight_remaining(deadline)?,
		MAX_PREFLIGHT_FILE_BYTES,
	)?;

	if !status.success() {
		return Err(SupervisionError::PreflightFailed.into());
	}

	let generated = GeneratedSchemaEvidence::load(schema_directory.path(), expected_digests)
		.map_err(|markers| ProbeError::SchemaMissing { markers })?;

	Ok((build, generated))
}

fn preflight_remaining(deadline: Instant) -> Result<Duration, SupervisionError> {
	let remaining = deadline.saturating_duration_since(Instant::now());

	if remaining.is_zero() { Err(SupervisionError::PreflightFailed) } else { Ok(remaining) }
}

fn run_preflight_command(
	command: &AppServerCommand,
	args: &[OsString],
	stdout: Stdio,
	timeout: Duration,
	max_file_bytes: u64,
) -> Result<ExitStatus, SupervisionError> {
	let mut process = Command::new(&command.program);

	process
		.args(args)
		.current_dir(&command.working_directory)
		.stdin(Stdio::null())
		.stdout(stdout)
		.stderr(Stdio::null());

	for name in CREDENTIAL_ENVIRONMENT {
		process.env_remove(name);
	}

	configure_process_group(&mut process, Some(max_file_bytes));

	let started = Instant::now();
	let term_deadline = started + timeout / 2;
	let kill_deadline = started + timeout * 3 / 4;
	let hard_deadline = started + timeout;
	let mut child = process.spawn().map_err(|_| SupervisionError::PreflightFailed)?;
	let mut status = None;
	let mut term_sent = false;
	let mut kill_sent = false;

	loop {
		if status.is_none() {
			status = child.try_wait().map_err(|_| SupervisionError::PreflightFailed)?;
		}

		let group_exists = process_group_exists(child.id())?;

		if let Some(status) = status
			&& !group_exists
		{
			return Ok(status);
		}

		let now = Instant::now();

		if !term_sent && (status.is_some() || now >= term_deadline) {
			signal_process_group(child.id(), SIGTERM)?;

			term_sent = true;
		}
		if !kill_sent && now >= kill_deadline {
			signal_process_group(child.id(), SIGKILL)?;

			kill_sent = true;
		}
		if now >= hard_deadline {
			return Err(SupervisionError::PreflightFailed);
		}

		thread::sleep(Duration::from_millis(10));
	}
}

fn validate_initialize(
	value: &InitializeResponse,
	binding: &AccountBinding,
) -> Result<(), ProbeError> {
	if value.codex_home.is_empty()
		|| value.platform_family.is_empty()
		|| value.platform_os.is_empty()
		|| value.user_agent.is_empty()
	{
		return Err(ProbeError::Supervision(SupervisionError::InvalidProtocol));
	}
	if binding.expected_codex_home.as_os_str() != OsStr::new(&value.codex_home) {
		return Err(ProbeError::Supervision(SupervisionError::CodexHomeMismatch));
	}

	Ok(())
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command, max_file_bytes: Option<u64>) {
	// SAFETY: `setsid` and `setrlimit` are async-signal-safe and access no parent memory.
	unsafe {
		command.pre_exec(move || {
			if libc::setsid() == -1 {
				return Err(std::io::Error::last_os_error());
			}

			if let Some(limit) = max_file_bytes {
				let limit = libc::rlimit { rlim_cur: limit, rlim_max: limit };

				if libc::setrlimit(RLIMIT_FSIZE, &limit) == -1 {
					return Err(std::io::Error::last_os_error());
				}
			}

			Ok(())
		});
	}
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command, _max_file_bytes: Option<u64>) {}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: i32) -> Result<(), SupervisionError> {
	let pid = i32::try_from(pid).map_err(|_| SupervisionError::ShutdownFailed)?;
	// SAFETY: a negative pid targets only the child-created session/process group.
	let result = unsafe { libc::kill(-pid, signal) };

	if result == 0 || io::Error::last_os_error().raw_os_error() == Some(ESRCH) {
		Ok(())
	} else {
		Err(SupervisionError::ShutdownFailed)
	}
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> Result<bool, SupervisionError> {
	let pid = i32::try_from(pid).map_err(|_| SupervisionError::ShutdownFailed)?;
	// SAFETY: signal zero performs existence/permission checking only.
	let result = unsafe { libc::kill(-pid, 0) };

	if result == 0 {
		return Ok(true);
	}

	match io::Error::last_os_error().raw_os_error() {
		Some(ESRCH) => Ok(false),
		Some(EPERM) => Ok(true),
		_ => Err(SupervisionError::ShutdownFailed),
	}
}

#[cfg(not(unix))]
fn signal_process_group(_pid: u32, _signal: i32) -> Result<(), SupervisionError> {
	Err(SupervisionError::ShutdownFailed)
}

#[cfg(not(unix))]
fn process_group_exists(_pid: u32) -> Result<bool, SupervisionError> {
	Ok(true)
}

#[cfg(test)]
mod tests {
	use std::{
		env, fs,
		io::Cursor,
		path::{Path, PathBuf},
		sync::{
			Arc,
			atomic::{AtomicBool, Ordering},
			mpsc,
		},
		thread,
		time::{Duration, Instant},
	};

	use tempfile::TempDir;

	use crate::{
		Capability, CapabilityCache, CapabilityState, ProbeError, SchemaMarker,
		process::{
			self, AccountBinding, AppServerCommand, PROTOCOL_QUEUE_CAPACITY, ReadOnlyProbe,
			ShutdownOutcome, SupervisedProcess, SupervisionError,
		},
	};

	fn fake_command(mode: &str, directory: &Path, extra: Option<&Path>) -> AppServerCommand {
		let fixture =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_app_server.py");
		let mut app_args = vec![fixture.clone().into_os_string(), "serve".into(), mode.into()];

		if let Some(extra) = extra {
			app_args.push(extra.as_os_str().to_owned());
		}

		let mut schema_args =
			vec![fixture.clone().into_os_string(), "generate-json-schema".into(), "--out".into()];

		if mode == "schema-missing" {
			schema_args.push("--missing-required".into());
		}
		if mode == "preflight-orphan" {
			schema_args.push("--orphan-pid".into());
			schema_args
				.push(extra.expect("preflight orphan needs pid path").as_os_str().to_owned());
		}
		if mode == "false-collaboration" {
			schema_args.push("--false-collaboration".into());
		}
		if mode == "oversized-schema" {
			schema_args.push("--oversized-schema".into());
		}

		let version_flag = match mode {
			"preflight-hang" => "--version-hang",
			"oversized-version" => "--version-oversized",
			_ => "--version",
		};

		AppServerCommand::new_for_test(
			"python3",
			app_args,
			[fixture.clone().into_os_string(), version_flag.into()],
			schema_args,
			directory,
		)
	}

	fn binding() -> AccountBinding {
		AccountBinding::for_test(PathBuf::from("/tmp/fake-codex-home"))
	}

	#[test]
	fn production_command_shape_cannot_inject_fake_launch_arguments() {
		let command = AppServerCommand::new("/tmp/project");

		assert_eq!(command.program, PathBuf::from("codex"));
		assert_eq!(command.app_server_args, ["app-server", "--stdio"]);
		assert_eq!(command.version_args, ["--version"]);
		assert_eq!(
			command.schema_args,
			["app-server", "generate-json-schema", "--experimental", "--out"]
		);
	}

	#[test]
	fn fake_probe_is_typed_and_preserves_one_account_binding() {
		let temp = TempDir::new().unwrap();
		let result = ReadOnlyProbe::new_for_test(
			fake_command("normal", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.profile.state(Capability::Initialize), &CapabilityState::Supported);
		assert_eq!(result.profile.state(Capability::ThreadList), &CapabilityState::Supported);
		assert_eq!(result.threads.len(), 1);
		assert_eq!(result.threads[0].id.as_str(), "00000000-0000-4000-8000-000000000001");
		assert!(result.account_identity.is_some());
		assert!(result.profile.build().as_str().starts_with("sha256:"));
		assert_eq!(result.profile.build().as_str().len(), 71);
		assert_eq!(result.profile.schema_fingerprint().len(), 64);
	}

	#[test]
	fn required_schema_failure_occurs_before_process_spawn() {
		let temp = TempDir::new().unwrap();
		let marker_path = temp.path().join("spawned");
		let mut marker = SchemaMarker::accepted();

		marker.remove_request_method("thread/list");

		let spawned = AtomicBool::new(false);
		let result = ReadOnlyProbe::new_for_test(
			fake_command("mark-spawn", temp.path(), Some(&marker_path)),
			binding(),
			marker,
			Duration::from_millis(100),
		)
		.run(&mut CapabilityCache::default());

		spawned.store(marker_path.exists(), Ordering::SeqCst);

		assert!(matches!(result, Err(ProbeError::SchemaMissing { .. })));
		assert!(!spawned.load(Ordering::SeqCst));
	}

	#[test]
	fn generated_schema_failure_occurs_before_app_server_spawn() {
		let temp = TempDir::new().unwrap();
		let marker_path = temp.path().join("spawned");
		let result = ReadOnlyProbe::new_for_test(
			fake_command("schema-missing", temp.path(), Some(&marker_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default());

		assert!(matches!(result, Err(ProbeError::SchemaMissing { .. })));
		assert!(!marker_path.exists());
	}

	#[test]
	fn timeout_and_crash_are_typed_without_raw_output() {
		let temp = TempDir::new().unwrap();

		for (mode, expected) in [
			("hang", SupervisionError::ResponseTimeout),
			("crash", SupervisionError::ProcessExited),
		] {
			let error = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(1),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(expected));
			assert!(!format!("{error:?}").contains("top-secret"));
			assert!(!format!("{error:?}").contains("sk-this-must-never-escape"));
		}
	}

	#[test]
	fn preflight_timeout_is_bounded() {
		let temp = TempDir::new().unwrap();
		let started = Instant::now();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("preflight-hang", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_millis(200),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		assert!(started.elapsed() < Duration::from_secs(1));
	}

	#[test]
	fn oversized_version_output_fails_closed_without_exporting_output() {
		let temp = TempDir::new().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("oversized-version", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		assert_eq!(format!("{error:?}"), "Supervision(PreflightFailed)");
	}

	#[test]
	fn structurally_false_collaboration_schema_fails_before_spawn() {
		let temp = TempDir::new().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("false-collaboration", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert!(matches!(error, ProbeError::SchemaMissing { .. }));
	}

	#[test]
	fn oversized_generated_schema_output_fails_closed() {
		let temp = TempDir::new().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("oversized-schema", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
	}

	#[test]
	fn oversized_no_newline_stdout_frame_fails_closed() {
		let temp = TempDir::new().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("oversized-frame", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ProtocolLimitExceeded));
	}

	#[test]
	fn stdout_queue_overflow_is_detected_without_blocking_the_reader() {
		let frames = vec![b'\n'; PROTOCOL_QUEUE_CAPACITY + 1];
		let (sender, _receiver) = mpsc::sync_channel(PROTOCOL_QUEUE_CAPACITY);
		let exceeded = Arc::new(AtomicBool::new(false));

		process::pump_stdout(Cursor::new(frames), sender, Arc::clone(&exceeded));

		assert!(exceeded.load(Ordering::Acquire));
	}

	#[test]
	fn oversized_thread_list_result_fails_closed() {
		let temp = TempDir::new().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("oversized-thread-list", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ProtocolLimitExceeded));
	}

	#[test]
	fn account_and_home_mismatch_fail_before_probe_exposure() {
		let temp = TempDir::new().unwrap();

		for (mode, expected) in [
			("account-none", SupervisionError::AccountUnavailable),
			("home-mismatch", SupervisionError::CodexHomeMismatch),
		] {
			let error = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(1),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(expected));
		}
	}

	#[cfg(unix)]
	#[test]
	fn preflight_cleans_descendants_before_app_server_spawn() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("preflight-descendant.pid");
		let result = ReadOnlyProbe::new_for_test(
			fake_command("preflight-orphan", temp.path(), Some(&pid_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();
		let descendant = read_pid(&pid_path);

		assert!(result.profile.build().as_str().starts_with("sha256:"));
		assert!(!process_exists(descendant));
	}

	#[test]
	#[ignore = "requires installed Codex; sends only schema/version and read-only RPCs"]
	fn live_read_only_probe_negotiates_without_dispatch() {
		let command = AppServerCommand::new(env::current_dir().unwrap());
		let result = ReadOnlyProbe::new(
			command,
			AccountBinding::shared_home().unwrap(),
			Duration::from_secs(10),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.profile.state(Capability::Initialize), &CapabilityState::Supported);
		assert_eq!(result.profile.state(Capability::ThreadList), &CapabilityState::Supported);
	}

	#[cfg(unix)]
	#[test]
	fn shutdown_kills_descendants_after_parent_exits_immediately() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("descendant.pid");
		let process = SupervisedProcess::spawn(
			fake_command("orphan-exit", temp.path(), Some(&pid_path)),
			binding(),
		)
		.unwrap();
		let deadline = Instant::now() + Duration::from_secs(2);

		while !pid_path.exists() && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		let descendant = read_pid(&pid_path);

		process.shutdown(Duration::from_secs(1)).unwrap();

		let deadline = Instant::now() + Duration::from_secs(2);

		while process_exists(descendant) && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		assert!(!process_exists(descendant), "descendant survived process-group shutdown");
	}

	#[cfg(unix)]
	#[test]
	fn stubborn_process_group_shutdown_is_bounded() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("descendant.pid");
		let process = SupervisedProcess::spawn(
			fake_command("orphan-stubborn", temp.path(), Some(&pid_path)),
			binding(),
		)
		.unwrap();
		let deadline = Instant::now() + Duration::from_secs(2);

		while !pid_path.exists() && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		let descendant = read_pid(&pid_path);
		let started = Instant::now();
		let outcome = process.shutdown(Duration::from_millis(500)).unwrap();

		assert_eq!(outcome, ShutdownOutcome::KilledAfterTimeout);
		assert!(started.elapsed() < Duration::from_secs(1));
		assert!(!process_exists(descendant));
	}

	#[cfg(unix)]
	fn process_exists(pid: i32) -> bool {
		// SAFETY: signal zero performs existence/permission checking only.
		unsafe { libc::kill(pid, 0) == 0 }
	}

	#[cfg(unix)]
	fn read_pid(path: &Path) -> i32 {
		let deadline = Instant::now() + Duration::from_secs(2);

		loop {
			if let Ok(pid) = fs::read_to_string(path)
				&& let Ok(pid) = pid.trim().parse()
			{
				return pid;
			}

			assert!(Instant::now() < deadline, "fake child pid was not written");

			thread::sleep(Duration::from_millis(10));
		}
	}
}
