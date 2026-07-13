use std::{
	collections::BTreeMap,
	env,
	ffi::{OsStr, OsString},
	fmt::{Debug, Display, Formatter},
	fs::{self, File},
	io::{self, BufRead, BufReader, Read as _, Write as _},
	os::unix::{fs::PermissionsExt as _, process::CommandExt as _},
	path::{Path, PathBuf},
	process::{Child, ChildStdin, Command, ExitStatus, Stdio},
	sync::{
		Arc, OnceLock,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError},
	},
	thread::{self, Builder},
	time::{Duration, Instant},
};

use libc::{EPERM, ESRCH, RLIMIT_FSIZE, SIGKILL, SIGTERM};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{self, Value};
use sha2::{Digest as _, Sha256};
use tempfile::{NamedTempFile, TempDir};

use crate::{
	capability::{
		Capability, CapabilityCache, CapabilityProfile, LiveMethodOutcome, MethodObservation,
		UnavailableReason,
	},
	protocol::{
		AccountReadResponse, BuildId, ClientInfo, InitializeCapabilities, InitializeParams,
		InitializeResponse, JsonRpcResponse, MAX_APP_SERVER_FRAME_BYTES, ThreadListParams,
		ThreadListResponse, ThreadReadParams, ThreadReadResponse, ThreadSearchParams,
		ThreadSearchResponse, ThreadSummary,
	},
	schema::{GeneratedSchemaEvidence, MAX_SCHEMA_FILE_BYTES, SchemaContract, SchemaMarker},
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
const PROTOCOL_QUEUE_CAPACITY: usize = 64;
const THREAD_LIST_LIMIT: usize = 100;
const THREAD_SEARCH_LIMIT: usize = 10;
const MAX_VERSION_OUTPUT_BYTES: u64 = 4 * 1_024;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1_024 * 1_024;
const CAPABILITY_SEARCH_TERM: &str = "decodex-capability-probe-7f57a41a";

/// Immutable shared-home account authority. There is intentionally no rebinding API.
#[derive(Clone)]
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

impl Debug for AccountBinding {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AccountBinding").finish_non_exhaustive()
	}
}

/// Exact executable contract for one supervised app-server build.
#[derive(Clone)]
pub struct AppServerCommand {
	program: PathBuf,
	executable_digest: [u8; 32],
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
	pub fn new(working_directory: impl Into<PathBuf>) -> Result<Self, SupervisionError> {
		let (program, executable_digest) = resolve_executable(OsStr::new("codex"))?;

		Ok(Self::production_from_resolved(program, executable_digest, working_directory.into()))
	}

	fn production_from_resolved(
		program: PathBuf,
		executable_digest: [u8; 32],
		working_directory: PathBuf,
	) -> Self {
		Self {
			program,
			executable_digest,
			app_server_args: vec!["app-server".into(), "--stdio".into()],
			version_args: vec!["--version".into()],
			schema_args: vec![
				"app-server".into(),
				"generate-json-schema".into(),
				"--experimental".into(),
				"--out".into(),
			],
			working_directory,
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
		let program = program.into();
		let (program, executable_digest) =
			resolve_executable(program.as_os_str()).expect("fake executable must resolve");

		Self {
			program,
			executable_digest,
			app_server_args: app_server_args.into_iter().map(Into::into).collect(),
			version_args: version_args.into_iter().map(Into::into).collect(),
			schema_args: schema_args.into_iter().map(Into::into).collect(),
			working_directory: working_directory.into(),
		}
	}
}

impl Debug for AppServerCommand {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AppServerCommand").finish_non_exhaustive()
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
	owner: ProcessGroupOwner,
	stdin: ChildStdin,
	stdout: Receiver<Vec<u8>>,
	protocol_limit_exceeded: Arc<AtomicBool>,
	binding: AccountBinding,
	command: AppServerCommand,
	expected_account_identity: Option<AccountIdentity>,
	next_request_id: u64,
}
impl SupervisedProcess {
	fn spawn(command: AppServerCommand, binding: AccountBinding) -> Result<Self, SupervisionError> {
		verify_executable(&command)?;

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

		let child = process.spawn().map_err(|_| SupervisionError::SpawnFailed)?;
		let mut owner = ProcessGroupOwner::new(child);
		let stdin = owner.child_mut().stdin.take().ok_or(SupervisionError::StdinUnavailable)?;
		let stdout = owner.child_mut().stdout.take().ok_or(SupervisionError::InvalidProtocol)?;
		let (sender, receiver) = mpsc::sync_channel(PROTOCOL_QUEUE_CAPACITY);
		let protocol_limit_exceeded = Arc::new(AtomicBool::new(false));
		let reader_limit_exceeded = Arc::clone(&protocol_limit_exceeded);

		thread::spawn(move || {
			pump_stdout(BufReader::new(stdout), sender, reader_limit_exceeded);
		});

		Ok(Self {
			owner,
			stdin,
			stdout: receiver,
			protocol_limit_exceeded,
			binding,
			command,
			expected_account_identity: None,
			next_request_id: 1,
		})
	}

	/// OS process identifier, used only for bounded supervision evidence.
	pub fn process_id(&self) -> u32 {
		self.owner.process_id()
	}

	/// Stop this child and its process group within a bounded deadline.
	pub fn shutdown(mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		self.shutdown_inner(timeout)
	}

	/// Restart using the same immutable account binding and exact command/build.
	pub fn restart(mut self, timeout: Duration) -> Result<Self, SupervisionError> {
		self.shutdown_inner(timeout)?;

		let expected_account_identity = self.expected_account_identity.clone();
		let mut restarted = Self::spawn(self.command.clone(), self.binding.clone())?;

		restarted.expected_account_identity = expected_account_identity;

		Ok(restarted)
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

	fn read_account_identity(&mut self, timeout: Duration) -> Result<AccountIdentity, ProbeError> {
		let account = self.request::<_, AccountReadResponse>(
			ReadOnlyMethod::AccountRead,
			&serde_json::json!({}),
			timeout,
		)?;
		let identity = account_identity(account)?;

		if self.expected_account_identity.as_ref().is_some_and(|expected| expected != &identity) {
			return Err(SupervisionError::AccountChanged.into());
		}

		self.expected_account_identity = Some(identity.clone());

		Ok(identity)
	}

	fn write_json(&mut self, value: &Value) -> Result<(), ProbeError> {
		let bytes = serde_json::to_vec(value)
			.map_err(|_| ProbeError::Supervision(SupervisionError::WriteFailed))?;

		if bytes.len().saturating_add(1) > MAX_APP_SERVER_FRAME_BYTES {
			return Err(SupervisionError::ProtocolLimitExceeded.into());
		}

		self.stdin
			.write_all(&bytes)
			.and_then(|_| self.stdin.write_all(b"\n"))
			.and_then(|_| self.stdin.flush())
			.map_err(|_| ProbeError::Supervision(SupervisionError::WriteFailed))
	}

	fn shutdown_inner(&mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		self.owner.shutdown(timeout)
	}
}

impl Debug for SupervisedProcess {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("SupervisedProcess").field("pid", &self.owner.process_id()).finish()
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
	/// Opaque active-account receipt, derived and re-attested around every authority read.
	pub account_identity: AccountIdentity,
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
		SchemaContract::validate(self.schema_marker.clone())
			.map_err(|markers| ProbeError::SchemaMissing { markers })?;

		let expected_digests =
			self.enforce_generated_digests.then_some(self.schema_marker.canonical_digests());
		let (build, generated) = attest_executable(&self.command, expected_digests, self.timeout)?;
		let mut negotiation = ProbeNegotiation::new(cache, &build, &generated);
		let mut process = match SupervisedProcess::spawn(self.command, self.binding) {
			Ok(process) => process,
			Err(error) => return negotiation.fail(Capability::Initialize, error.into()),
		};
		let account_identity = initialize_probe(&mut process, self.timeout, &mut negotiation)?;
		let list = probe_thread_list(&mut process, self.timeout, &mut negotiation)?;

		probe_thread_read(&mut process, &list, self.timeout, &mut negotiation)?;
		probe_thread_search(&mut process, self.timeout, &mut negotiation)?;

		let profile = negotiation.cache_profile()?;

		process.shutdown(Duration::from_secs(1))?;

		Ok(ReadOnlyProbeResult {
			profile,
			threads: list.data.into_iter().map(ThreadSummary::from).collect(),
			account_identity,
		})
	}
}

struct ProbeNegotiation<'a> {
	cache: &'a mut CapabilityCache,
	build: &'a BuildId,
	generated: &'a GeneratedSchemaEvidence,
	observations: Vec<MethodObservation>,
}
impl<'a> ProbeNegotiation<'a> {
	fn new(
		cache: &'a mut CapabilityCache,
		build: &'a BuildId,
		generated: &'a GeneratedSchemaEvidence,
	) -> Self {
		Self { cache, build, generated, observations: Vec::new() }
	}

	fn observe(&mut self, capability: Capability, outcome: LiveMethodOutcome) {
		self.observations.push(MethodObservation { capability, outcome });
	}

	fn fail<T>(&mut self, capability: Capability, error: ProbeError) -> Result<T, ProbeError> {
		self.observations.push(failed_observation(capability, &error));
		self.cache_profile()?;

		Err(error)
	}

	fn cache_profile(&mut self) -> Result<CapabilityProfile, ProbeError> {
		cache_profile(self.cache, self.build, self.generated, &self.observations)
	}

	fn re_attest_account(
		&mut self,
		process: &mut SupervisedProcess,
		timeout: Duration,
	) -> Result<(), ProbeError> {
		match process.read_account_identity(timeout) {
			Ok(_) => Ok(()),
			Err(error @ ProbeError::Supervision(SupervisionError::AccountChanged)) => Err(error),
			Err(error) => self.fail(Capability::AccountRead, error),
		}
	}
}

struct ReapJob {
	child: Child,
	process_group: u32,
}

struct ProcessGroupOwner {
	child: Option<Child>,
	process_group: u32,
}
impl ProcessGroupOwner {
	fn new(child: Child) -> Self {
		let process_group = child.id();

		Self { child: Some(child), process_group }
	}

	fn child_mut(&mut self) -> &mut Child {
		self.child.as_mut().expect("live owner must retain its child")
	}

	fn process_id(&self) -> u32 {
		self.process_group
	}

	fn shutdown(&mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		let Some(child) = self.child.as_mut() else {
			return Ok(ShutdownOutcome::Exited);
		};
		let outcome = terminate_process_group(child, self.process_group, timeout)?;
		let mut child = self.child.take().expect("confirmed child was present");
		let _ = child.wait();

		Ok(outcome)
	}

	fn transfer_to_reaper(&mut self) {
		let Some(child) = self.child.take() else {
			return;
		};
		let job = ReapJob { child, process_group: self.process_group };

		match process_reaper() {
			Some(reaper) =>
				if let Err(error) = reaper.send(job) {
					reap_process_group(error.0);
				},
			None => reap_process_group(job),
		}
	}
}

impl Drop for ProcessGroupOwner {
	fn drop(&mut self) {
		if self.child.is_some() && self.shutdown(Duration::from_millis(250)).is_err() {
			self.transfer_to_reaper();
		}
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
	/// The account identity changed while capability authority was being read.
	AccountChanged,
	/// The configured executable could not be resolved to an executable regular file.
	ExecutableUnavailable,
	/// Executable contents changed after command construction.
	ExecutableChanged,
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
	/// Read one exact thread without turns.
	ThreadRead,
	/// Exercise bounded `thread/search` method availability.
	ThreadSearch,
}
impl ReadOnlyMethod {
	fn as_str(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountRead => "account/read",
			Self::ThreadList => "thread/list",
			Self::ThreadRead => "thread/read",
			Self::ThreadSearch => "thread/search",
		}
	}
}

fn resolve_executable(program: &OsStr) -> Result<(PathBuf, [u8; 32]), SupervisionError> {
	let requested = Path::new(program);
	let candidate = if requested.components().count() > 1 {
		requested.to_owned()
	} else {
		let path = env::var_os("PATH").ok_or(SupervisionError::ExecutableUnavailable)?;

		env::split_paths(&path)
			.map(|directory| directory.join(requested))
			.find(|candidate| candidate.is_file())
			.ok_or(SupervisionError::ExecutableUnavailable)?
	};
	let canonical =
		candidate.canonicalize().map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let digest = executable_digest(&canonical)?;

	Ok((canonical, digest))
}

fn executable_digest(path: &Path) -> Result<[u8; 32], SupervisionError> {
	let metadata = path.symlink_metadata().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	if metadata.file_type().is_symlink()
		|| !metadata.is_file()
		|| metadata.permissions().mode() & 0o111 == 0
		|| metadata.len() > MAX_EXECUTABLE_BYTES
	{
		return Err(SupervisionError::ExecutableUnavailable);
	}

	let mut file = File::open(path).map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let mut hasher = Sha256::new();
	let mut remaining = MAX_EXECUTABLE_BYTES + 1;
	let mut buffer = [0_u8; 64 * 1_024];

	while remaining > 0 {
		let limit = usize::try_from(remaining.min(buffer.len() as u64))
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;
		let count =
			file.read(&mut buffer[..limit]).map_err(|_| SupervisionError::ExecutableUnavailable)?;

		if count == 0 {
			break;
		}

		hasher.update(&buffer[..count]);

		remaining -= count as u64;
	}

	if remaining == 0 {
		return Err(SupervisionError::ExecutableUnavailable);
	}

	Ok(hasher.finalize().into())
}

fn verify_executable(command: &AppServerCommand) -> Result<(), SupervisionError> {
	let canonical =
		command.program.canonicalize().map_err(|_| SupervisionError::ExecutableChanged)?;

	if canonical != command.program
		|| executable_digest(&canonical).map_err(|_| SupervisionError::ExecutableChanged)?
			!= command.executable_digest
	{
		return Err(SupervisionError::ExecutableChanged);
	}

	Ok(())
}

fn process_reaper() -> Option<&'static Sender<ReapJob>> {
	static REAPER: OnceLock<Option<Sender<ReapJob>>> = OnceLock::new();

	REAPER
		.get_or_init(|| {
			let (sender, receiver) = mpsc::channel::<ReapJob>();

			Builder::new()
				.name("decodex-codex-process-reaper".into())
				.spawn(move || {
					for job in receiver {
						reap_process_group(job);
					}
				})
				.ok()
				.map(|_| sender)
		})
		.as_ref()
}

fn reap_process_group(mut job: ReapJob) {
	loop {
		let _ = job.child.try_wait();

		if matches!(process_group_exists(job.process_group), Ok(false)) {
			let _ = job.child.wait();

			break;
		}

		let _ = signal_process_group(job.process_group, SIGKILL);

		thread::sleep(Duration::from_millis(25));
	}
}

fn account_identity(response: AccountReadResponse) -> Result<AccountIdentity, ProbeError> {
	let account = response.account.ok_or(SupervisionError::AccountUnavailable)?;
	let mut digest = Sha256::new();

	digest.update(account.kind.as_bytes());
	digest.update([0]);

	if let Some(email) = account.email {
		digest.update(email.as_bytes());
	}

	digest.update([u8::from(response.requires_openai_auth)]);

	let digest = digest.finalize();
	let hex = b"0123456789abcdef";
	let mut identity = String::with_capacity(71);

	identity.push_str("sha256:");

	for byte in digest {
		identity.push(char::from(hex[usize::from(byte >> 4)]));
		identity.push(char::from(hex[usize::from(byte & 0x0f)]));
	}

	Ok(AccountIdentity(identity))
}

fn failed_observation(capability: Capability, error: &ProbeError) -> MethodObservation {
	let outcome = match error {
		ProbeError::MethodRejected { code, .. } => LiveMethodOutcome::Unsupported { code: *code },
		_ => LiveMethodOutcome::Unavailable { reason: UnavailableReason::ProbeFailed },
	};

	MethodObservation { capability, outcome }
}

fn cache_profile(
	cache: &mut CapabilityCache,
	build: &BuildId,
	generated: &GeneratedSchemaEvidence,
	observations: &[MethodObservation],
) -> Result<CapabilityProfile, ProbeError> {
	let profile = CapabilityProfile::negotiate(
		build.clone(),
		generated.fingerprint.clone(),
		generated.contract(),
		observations.iter().cloned(),
	);

	cache.insert(profile).map_err(|_| ProbeError::CapabilityConflict)
}

fn initialize_probe(
	process: &mut SupervisedProcess,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<AccountIdentity, ProbeError> {
	let initialize = match process.request::<_, InitializeResponse>(
		ReadOnlyMethod::Initialize,
		&InitializeParams {
			client_info: ClientInfo { name: "decodex", version: env!("CARGO_PKG_VERSION") },
			capabilities: InitializeCapabilities { experimental_api: true },
		},
		timeout,
	) {
		Ok(initialize) => initialize,
		Err(error) => return negotiation.fail(Capability::Initialize, error),
	};

	if let Err(error) = validate_initialize(&initialize, &process.binding) {
		return negotiation.fail(Capability::Initialize, error);
	}

	negotiation.observe(Capability::Initialize, LiveMethodOutcome::Supported);

	if let Err(error) = process.notify("initialized", &serde_json::json!({})) {
		return negotiation.fail(Capability::Initialize, error);
	}

	let identity = match process.read_account_identity(timeout) {
		Ok(identity) => identity,
		Err(error) => return negotiation.fail(Capability::AccountRead, error),
	};

	negotiation.observe(Capability::AccountRead, LiveMethodOutcome::Supported);

	Ok(identity)
}

fn probe_thread_list(
	process: &mut SupervisedProcess,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<ThreadListResponse, ProbeError> {
	let list = match process.request::<_, ThreadListResponse>(
		ReadOnlyMethod::ThreadList,
		&ThreadListParams { limit: THREAD_LIST_LIMIT as u32, use_state_db_only: true },
		timeout,
	) {
		Ok(list) => list,
		Err(error) => return negotiation.fail(Capability::ThreadList, error),
	};

	if list.data.len() > THREAD_LIST_LIMIT {
		return negotiation
			.fail(Capability::ThreadList, SupervisionError::ProtocolLimitExceeded.into());
	}

	negotiation.observe(Capability::ThreadList, LiveMethodOutcome::Supported);
	negotiation.re_attest_account(process, timeout)?;

	Ok(list)
}

fn probe_thread_read(
	process: &mut SupervisedProcess,
	list: &ThreadListResponse,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<(), ProbeError> {
	if !negotiation.generated.contract().advertises_request("thread/read") {
		return Ok(());
	}

	let Some(thread) = list.data.first() else {
		return Ok(());
	};
	let result = process.request::<_, ThreadReadResponse>(
		ReadOnlyMethod::ThreadRead,
		&ThreadReadParams { thread_id: &thread.id, include_turns: false },
		timeout,
	);
	let terminal_error = match result {
		Ok(response) if response.thread.id == thread.id => {
			negotiation.observe(Capability::ThreadRead, LiveMethodOutcome::Supported);

			None
		},
		Ok(_) => {
			let error = ProbeError::Supervision(SupervisionError::InvalidProtocol);

			negotiation.observations.push(failed_observation(Capability::ThreadRead, &error));

			Some(error)
		},
		Err(error) => {
			negotiation.observations.push(failed_observation(Capability::ThreadRead, &error));

			(!matches!(&error, ProbeError::MethodRejected { .. })).then_some(error)
		},
	};

	negotiation.re_attest_account(process, timeout)?;

	if let Some(error) = terminal_error {
		negotiation.cache_profile()?;

		return Err(error);
	}

	Ok(())
}

fn probe_thread_search(
	process: &mut SupervisedProcess,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<(), ProbeError> {
	if !negotiation.generated.contract().advertises_request("thread/search") {
		return Ok(());
	}

	let search = process.request::<_, ThreadSearchResponse>(
		ReadOnlyMethod::ThreadSearch,
		&ThreadSearchParams {
			search_term: CAPABILITY_SEARCH_TERM,
			limit: THREAD_SEARCH_LIMIT as u32,
		},
		timeout,
	);
	let terminal_error = match search {
		Ok(response) if response.data.len() <= THREAD_SEARCH_LIMIT => {
			negotiation.observe(Capability::ThreadSearch, LiveMethodOutcome::Supported);

			None
		},
		Ok(_) => {
			let error = ProbeError::Supervision(SupervisionError::ProtocolLimitExceeded);

			negotiation.observations.push(failed_observation(Capability::ThreadSearch, &error));

			Some(error)
		},
		Err(error) => {
			negotiation.observations.push(failed_observation(Capability::ThreadSearch, &error));

			(!matches!(&error, ProbeError::MethodRejected { .. })).then_some(error)
		},
	};

	negotiation.re_attest_account(process, timeout)?;

	if let Some(error) = terminal_error {
		negotiation.cache_profile()?;

		return Err(error);
	}

	Ok(())
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
		match read_bounded_line(&mut reader, MAX_APP_SERVER_FRAME_BYTES) {
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
	let build = BuildId::from_attestation(version.trim(), &command.executable_digest)
		.map_err(|_| SupervisionError::PreflightFailed)?;
	let schema_directory = TempDir::new().map_err(|_| SupervisionError::PreflightFailed)?;
	let mut schema_args = command.schema_args.clone();

	schema_args.push(schema_directory.path().as_os_str().to_owned());

	let status = run_preflight_command(
		command,
		&schema_args,
		Stdio::null(),
		preflight_remaining(deadline)?,
		MAX_SCHEMA_FILE_BYTES,
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
	verify_executable(command)?;

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
	let child = process.spawn().map_err(|_| SupervisionError::PreflightFailed)?;
	let mut owner = ProcessGroupOwner::new(child);
	let mut status = None;
	let mut term_sent = false;
	let mut kill_sent = false;

	loop {
		if status.is_none() {
			status = owner.child_mut().try_wait().map_err(|_| SupervisionError::PreflightFailed)?;
		}

		let group_exists = process_group_exists(owner.process_id())?;

		if let Some(status) = status
			&& !group_exists
		{
			owner.shutdown(Duration::from_millis(1))?;

			return Ok(status);
		}

		let now = Instant::now();

		if !term_sent && (status.is_some() || now >= term_deadline) {
			signal_process_group(owner.process_id(), SIGTERM)?;

			term_sent = true;
		}
		if !kill_sent && now >= kill_deadline {
			signal_process_group(owner.process_id(), SIGKILL)?;

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

fn terminate_process_group(
	child: &mut Child,
	pid: u32,
	timeout: Duration,
) -> Result<ShutdownOutcome, SupervisionError> {
	let leader_exited = child.try_wait().map_err(|_| SupervisionError::ShutdownFailed)?.is_some();

	if !process_group_exists(pid)? {
		return Ok(ShutdownOutcome::Exited);
	}

	signal_process_group(pid, SIGTERM)?;

	let started = Instant::now();
	let term_deadline = started + timeout / 2;
	let hard_deadline = started + timeout;

	while Instant::now() < term_deadline {
		let _ = child.try_wait().map_err(|_| SupervisionError::ShutdownFailed)?;

		if !process_group_exists(pid)? {
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
		let _ = child.try_wait().map_err(|_| SupervisionError::ShutdownFailed)?;

		if !process_group_exists(pid)? {
			return Ok(ShutdownOutcome::KilledAfterTimeout);
		}

		thread::sleep(Duration::from_millis(10));
	}

	Err(SupervisionError::ShutdownFailed)
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
		os::unix::fs::PermissionsExt as _,
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
			self, AccountBinding, AppServerCommand, PROTOCOL_QUEUE_CAPACITY, ReadOnlyMethod,
			ReadOnlyProbe, ShutdownOutcome, SupervisedProcess, SupervisionError,
		},
		protocol::{ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse},
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
		if matches!(mode, "preflight-orphan" | "preflight-orphan-error") {
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
		if mode == "missing-optional" {
			schema_args.push("--missing-optional".into());
		}
		if mode == "missing-optional-methods" {
			schema_args.push("--missing-optional-methods".into());
		}
		if mode == "malformed-optional" {
			schema_args.push("--malformed-optional".into());
		}
		if mode == "too-many-schema-files" {
			schema_args.push("--too-many-files".into());
		}
		if mode == "schema-symlink" {
			schema_args.push("--schema-symlink".into());
		}
		if mode == "preflight-orphan-error" {
			schema_args.push("--preflight-fail".into());
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
		let command = AppServerCommand::production_from_resolved(
			PathBuf::from("/resolved/codex"),
			[7; 32],
			PathBuf::from("/tmp/project"),
		);

		assert!(command.program.is_absolute());
		assert_eq!(command.program.file_name().unwrap(), "codex");
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
		assert!(result.account_identity.as_str().starts_with("sha256:"));
		assert!(result.profile.build().as_str().starts_with("sha256:"));
		assert_eq!(result.profile.build().as_str().len(), 71);
		assert_eq!(result.profile.schema_fingerprint().len(), 64);
		assert_eq!(
			result.profile.state(Capability::PaginatedHistory),
			&CapabilityState::Unavailable { reason: crate::UnavailableReason::NotProbed }
		);
		assert_eq!(
			result.profile.state(Capability::NativeCollaboration),
			&CapabilityState::Unavailable { reason: crate::UnavailableReason::NotProbed }
		);
	}

	#[test]
	fn safe_optional_probes_record_live_contradictions_by_exact_build() {
		let temp = TempDir::new().unwrap();
		let result = ReadOnlyProbe::new_for_test(
			fake_command("optional-unsupported", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		for capability in [Capability::ThreadRead, Capability::ThreadSearch] {
			assert_eq!(
				result.profile.state(capability),
				&CapabilityState::Unsupported { reason: crate::UnsupportedReason::MethodNotFound }
			);
			assert!(
				result
					.profile
					.contradictions()
					.iter()
					.any(|contradiction| contradiction.capability == capability)
			);
		}
	}

	#[test]
	fn missing_optional_methods_and_schema_degrade_without_failing_preflight() {
		for (mode, capabilities) in [
			("missing-optional-methods", vec![Capability::ThreadRead, Capability::ThreadSearch]),
			(
				"missing-optional",
				vec![Capability::PaginatedHistory, Capability::NativeCollaboration],
			),
			(
				"malformed-optional",
				vec![Capability::PaginatedHistory, Capability::NativeCollaboration],
			),
		] {
			let temp = TempDir::new().unwrap();
			let result = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run(&mut CapabilityCache::default())
			.unwrap();

			for capability in capabilities {
				assert_eq!(
					result.profile.state(capability),
					&CapabilityState::Unsupported {
						reason: crate::UnsupportedReason::SchemaMissing
					}
				);
			}
		}
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
	fn generated_schema_digest_mismatch_occurs_before_app_server_spawn() {
		let temp = TempDir::new().unwrap();
		let marker_path = temp.path().join("spawned");
		let error = ReadOnlyProbe::new(
			fake_command("mark-spawn", temp.path(), Some(&marker_path)),
			binding(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert!(matches!(error, ProbeError::SchemaMissing { .. }));
		assert!(!marker_path.exists());
	}

	#[test]
	fn generated_schema_file_count_and_symlinks_fail_before_app_server_spawn() {
		for mode in ["too-many-schema-files", "schema-symlink"] {
			let temp = TempDir::new().unwrap();
			let marker_path = temp.path().join("spawned");
			let error = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), Some(&marker_path)),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert!(matches!(error, ProbeError::SchemaMissing { .. }));
			assert!(!marker_path.exists());
		}
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
	fn post_attestation_failures_leave_a_sanitized_exact_build_profile() {
		let temp = TempDir::new().unwrap();
		let mut cache = CapabilityCache::default();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("crash", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut cache)
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ProcessExited));
		assert_eq!(cache.len(), 1);
		assert!(!format!("{cache:?}").contains("sk-this-must-never-escape"));
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
	fn structurally_false_collaboration_schema_degrades_without_a_live_claim() {
		let temp = TempDir::new().unwrap();
		let result = ReadOnlyProbe::new_for_test(
			fake_command("false-collaboration", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert_eq!(
			result.profile.state(Capability::NativeCollaboration),
			&CapabilityState::Unsupported { reason: crate::UnsupportedReason::SchemaMissing }
		);
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
	fn outbound_json_rpc_frames_are_bounded_before_write() {
		let temp = TempDir::new().unwrap();
		let mut process =
			SupervisedProcess::spawn(fake_command("normal", temp.path(), None), binding()).unwrap();
		let oversized =
			serde_json::json!({"value": "x".repeat(process::MAX_APP_SERVER_FRAME_BYTES)});

		assert_eq!(
			process.write_json(&oversized),
			Err(ProbeError::Supervision(SupervisionError::ProtocolLimitExceeded))
		);
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

	#[test]
	fn account_change_discards_the_in_progress_negotiation() {
		let temp = TempDir::new().unwrap();
		let mut cache = CapabilityCache::default();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("account-switch", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut cache)
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::AccountChanged));
		assert!(cache.is_empty());
	}

	#[test]
	fn restart_preserves_and_rechecks_the_expected_account_identity() {
		let temp = TempDir::new().unwrap();
		let timeout = Duration::from_secs(1);
		let mut process =
			SupervisedProcess::spawn(fake_command("account-switch", temp.path(), None), binding())
				.unwrap();

		initialize_test_process(&mut process, timeout);

		let initial = process.read_account_identity(timeout).unwrap();
		let mut restarted = process.restart(timeout).unwrap();

		assert_eq!(restarted.expected_account_identity.as_ref(), Some(&initial));

		initialize_test_process(&mut restarted, timeout);

		assert_eq!(restarted.read_account_identity(timeout).unwrap(), initial);
		assert_eq!(
			restarted.read_account_identity(timeout),
			Err(ProbeError::Supervision(SupervisionError::AccountChanged))
		);
	}

	#[test]
	fn debug_output_redacts_binding_command_and_process_paths() {
		let temp = TempDir::new().unwrap();
		let secret = "private-home-and-project";
		let binding = AccountBinding::for_test(PathBuf::from(format!("/tmp/{secret}/.codex")));
		let working_directory = temp.path().join(secret);

		fs::create_dir(&working_directory).unwrap();

		let command = fake_command("normal", &working_directory, None);
		let process = SupervisedProcess::spawn(command.clone(), binding.clone()).unwrap();

		for debug in [format!("{binding:?}"), format!("{command:?}"), format!("{process:?}")] {
			assert!(!debug.contains(secret));
			assert!(!debug.contains("private@example.test"));
		}
	}

	#[test]
	fn executable_content_replacement_invalidates_all_spawn_authority() {
		let temp = TempDir::new().unwrap();
		let fixture =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_app_server.py");
		let executable = temp.path().join("fake-codex");

		fs::copy(fixture, &executable).unwrap();
		fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

		let command = AppServerCommand::new_for_test(
			&executable,
			["serve", "normal"],
			["--version"],
			["generate-json-schema", "--out"],
			temp.path(),
		);

		fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
		fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

		let error = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ExecutableChanged));
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

	#[cfg(unix)]
	#[test]
	fn failed_preflight_keeps_descendant_authority_until_death() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("preflight-error-descendant.pid");
		let error = ReadOnlyProbe::new_for_test(
			fake_command("preflight-orphan-error", temp.path(), Some(&pid_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();
		let descendant = read_pid(&pid_path);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));

		wait_until_process_is_dead(descendant);
	}

	#[test]
	#[ignore = "requires installed Codex; sends only schema/version and read-only RPCs"]
	fn live_read_only_probe_negotiates_without_dispatch() {
		let command = AppServerCommand::new(env::current_dir().unwrap()).unwrap();
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
	fn drop_keeps_process_group_authority_until_descendants_die() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("drop-descendant.pid");
		let process = SupervisedProcess::spawn(
			fake_command("orphan-stubborn", temp.path(), Some(&pid_path)),
			binding(),
		)
		.unwrap();
		let descendant = read_pid(&pid_path);

		drop(process);
		wait_until_process_is_dead(descendant);
	}

	#[cfg(unix)]
	#[test]
	fn probe_error_keeps_process_group_authority_until_descendants_die() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("error-descendant.pid");
		let error = ReadOnlyProbe::new_for_test(
			fake_command("orphan-error", temp.path(), Some(&pid_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(1),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();
		let descendant = read_pid(&pid_path);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::InvalidProtocol));

		wait_until_process_is_dead(descendant);
	}

	#[cfg(unix)]
	#[test]
	fn probe_timeout_keeps_process_group_authority_until_descendants_die() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("timeout-descendant.pid");
		let error = ReadOnlyProbe::new_for_test(
			fake_command("orphan-timeout", temp.path(), Some(&pid_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_millis(250),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();
		let descendant = read_pid(&pid_path);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ResponseTimeout));

		wait_until_process_is_dead(descendant);
	}

	#[cfg(unix)]
	#[test]
	fn failed_bounded_shutdown_transfers_authority_to_the_owned_reaper() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("reaper-descendant.pid");
		let process = SupervisedProcess::spawn(
			fake_command("orphan-stubborn", temp.path(), Some(&pid_path)),
			binding(),
		)
		.unwrap();
		let descendant = read_pid(&pid_path);

		assert_eq!(process.shutdown(Duration::ZERO), Err(SupervisionError::ShutdownFailed));

		wait_until_process_is_dead(descendant);
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
	fn wait_until_process_is_dead(pid: i32) {
		let deadline = Instant::now() + Duration::from_secs(3);

		while process_exists(pid) && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		assert!(!process_exists(pid), "descendant survived owned process-group cleanup");
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

	fn initialize_test_process(process: &mut SupervisedProcess, timeout: Duration) {
		let initialize = process
			.request::<_, InitializeResponse>(
				ReadOnlyMethod::Initialize,
				&InitializeParams {
					client_info: ClientInfo { name: "decodex-test", version: "0" },
					capabilities: InitializeCapabilities { experimental_api: true },
				},
				timeout,
			)
			.unwrap();

		process::validate_initialize(&initialize, &process.binding).unwrap();

		process.notify("initialized", &serde_json::json!({})).unwrap();
	}
}
