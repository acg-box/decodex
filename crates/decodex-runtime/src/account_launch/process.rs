#[cfg(target_os = "linux")] use std::os::fd::{AsRawFd as _, FromRawFd as _};
#[cfg(test)] use std::sync::atomic::AtomicU32;
use std::{
	cell::UnsafeCell,
	collections::BTreeSet,
	env,
	ffi::{OsStr, OsString},
	fmt::{Debug, Display, Formatter},
	fs::{self, File, Metadata},
	io::{self, ErrorKind, Read, Write},
	mem::{self, MaybeUninit},
	os::unix::{
		ffi::OsStrExt as _,
		fs::{FileExt as _, MetadataExt as _, PermissionsExt as _},
		process::CommandExt as _,
	},
	panic::{self, AssertUnwindSafe},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	str,
	sync::{
		Arc, Condvar, Mutex, PoisonError,
		atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
		mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
	},
	thread::{self, Builder, JoinHandle, ThreadId},
	time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use libc::{EPERM, ESRCH, F_GETFL, F_SETFL, O_NONBLOCK, SIGKILL, SIGTERM};
#[cfg(target_os = "linux")]
use libc::{
	F_ADD_SEALS, F_GET_SEALS, F_SEAL_EXEC, F_SEAL_GROW, F_SEAL_SEAL, F_SEAL_SHRINK, F_SEAL_WRITE,
	MFD_ALLOW_SEALING, MFD_CLOEXEC, MFD_EXEC,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{self};
use sha2::{Digest as _, Sha256};
use tempfile::{NamedTempFile, TempDir};
use zeroize::{Zeroize as _, Zeroizing};
#[cfg(target_os = "macos")]
use {
	libc::UF_IMMUTABLE,
	std::ffi::CString,
	std::fs::{OpenOptions, Permissions},
	std::os::unix::fs::OpenOptionsExt as _,
};

#[cfg(test)] use crate::account_launch::RunnerCapacity;
#[cfg(target_os = "macos")]
use crate::account_launch::macos_attested_spawn::{
	AttestedChild, AttestedCodeIdentity, PRIVATE_STDIO_STARTUP_ENV, PRIVATE_STDIO_STARTUP_VALUE,
	spawn_private_stdio_suspended, spawn_private_stdio_suspended_at, spawn_suspended,
};
use crate::account_launch::{
	RunnerPermit,
	protocol::{
		AccountReadResponse, ClientInfo, ExactThreadListParams, ExactThreadReadParams,
		InitializeCapabilities, InitializeParams, InitializeResponse, JsonRpcResponse,
		MAX_APP_SERVER_FRAME_BYTES, ThreadArchiveParams, ThreadArchiveResponse, ThreadListParams,
		ThreadListResponse, ThreadReadParams, ThreadReadResponse,
	},
};
#[cfg(test)] use decodex_codex::schema::SchemaMarker;
use decodex_codex::{
	ArchiveReconciliationOutcome, ArchiveUnverifiedReason, BuildId, Capability, CapabilityCache,
	CapabilityProfile, ExactResetCreditId, ExactThreadFacts, ExactThreadId, ExactThreadListFilter,
	ExactThreadListResult, ExactThreadReadResult, LiveMethodOutcome, LossyThreadHistory,
	MAX_EXACT_THREAD_LIST_RESULTS, MethodObservation, NormalizedEvent, QuickTaskMessageDelta,
	QuickTaskThreadResumeRequest, QuickTaskThreadStartRequest, QuickTaskTurnInterruptRequest,
	QuickTaskTurnInterruptResponse, QuickTaskTurnStartRequest, RESET_CARD_CONSUME_METHOD,
	RESET_CARD_READ_METHOD, ResetCardCapabilityProfile, ResetCardCapabilityState,
	ResetCardConsumeParams, ResetCardConsumeResult, ResetCardIdempotencyKey, ResetCardInventory,
	ThreadSummary, TurnStatus, UnavailableReason, decode_quick_task_thread_resume_response,
	decode_quick_task_thread_start_response, decode_quick_task_turn_interrupt_response,
	decode_quick_task_turn_start_response, normalize_event, project_quick_task_message_delta,
	schema::{GeneratedSchemaEvidence, MAX_SCHEMA_FILE_BYTES},
};
use decodex_core::{
	AccountId, ProcessBootIdentity, ProcessControlKind, ProcessExecutionAuthorization,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationIntent,
	ProcessIsolationKind, ProcessRunnerIdentity, ProviderAttemptId, ResetCardConsumeOutcome,
};
use decodex_postgres::{
	BindRuntimeSessionThread, CodexAccountCapabilityAttestation, FreshProviderDispatchFence,
	FreshQuickTaskProcessGeneration, FreshRuntimeSessionThreadStart,
	SuccessfulRuntimeSessionThreadStart,
};
use decodex_protocol::MAX_QUICK_TASK_WORKING_DIRECTORY_BYTES;

use crate::process_supervisor::{FencedProcess, ProcessGenerationControl, ProcessSupervisorError};

/// Hard mechanical bound for process groups awaiting confirmed cleanup.
pub const MAX_PROCESS_QUARANTINE: usize = 64;

const CHILD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
#[cfg(not(target_os = "macos"))]
const PRIVATE_STDIO_STARTUP_ENV: &str = "CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED";
#[cfg(not(target_os = "macos"))]
const PRIVATE_STDIO_STARTUP_VALUE: &str = "1";
const PRIVATE_STDIO_CAPABILITY_ID: &str =
	"codex-app-server-private-stdio-disabled-ephemeral-startup-v1";
const STDIO_ONLY_ATTESTED_PLATFORM: &str = "macos-aarch64";
const PROTOCOL_QUEUE_CAPACITY: usize = 64;
const MAX_QUICK_TASK_BUFFERED_EVENTS: usize = PROTOCOL_QUEUE_CAPACITY;
const THREAD_LIST_LIMIT: usize = 100;
const THREAD_LIST_PROBE_SEARCH_TERM: &str = "decodex-capability-probe-no-match-6f5aa91b28cf4bc6";
const MAX_VERSION_OUTPUT_BYTES: u64 = 4 * 1_024;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1_024 * 1_024;
const ACCOUNT_MISMATCH_SHUTDOWN: Duration = Duration::from_secs(1);
const INBOUND_BLOCK_BYTES: usize = 8 * 1_024;
const OUTBOUND_BLOCK_BYTES: usize = 8 * 1_024;
const QUARANTINE_SLOT_FREE: u8 = 0;
const QUARANTINE_SLOT_RESERVED: u8 = 1;
const QUARANTINE_SLOT_READY: u8 = 2;
const QUARANTINE_SLOT_WORKING: u8 = 3;
const QUARANTINE_SHUTDOWN_WAIT: Duration = Duration::from_secs(1);
const RESET_CARD_SHUTDOWN_WAIT: Duration = Duration::from_secs(1);
const CALLBACK_PROBE_PLAN_TYPE: &str = "business";
const CALLBACK_PROBE_EMAIL: &str = "decodex-callback-capability-probe.invalid";
const CALLBACK_PROBE_SIGNATURE: &[u8] = b"decodex-callback-capability-probe";
#[cfg(test)]
static ZEROIZED_INBOUND_BLOCKS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static ZEROIZED_OUTBOUND_BLOCKS: AtomicUsize = AtomicUsize::new(0);

/// Host credential-vault port. Credential material stays inside this call and the
/// process projection sink; it is never returned to the caller.
pub(crate) trait CredentialVault: Send + Sync {
	/// Project exactly the caller-selected account into one not-yet-bound child.
	fn project(
		&self,
		account_id: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError>;
}

/// Process-bound service for the exact Codex ChatGPT refresh server request.
pub(crate) trait AccountRefreshCallback: Send + Sync {
	/// Serialize one callback through the daemon Account Service and return only response fields.
	fn refresh(
		&self,
		account_id: &AccountId,
		initial_binding: &ProcessGenerationAccountBinding,
		reason: &str,
		previous_provider_account_id: Option<&str>,
	) -> Result<ChatgptRefreshProjection, CredentialVaultError>;
}

/// Secret-bearing callback response retained only through one zeroizing JSON write.
pub(crate) struct ChatgptRefreshProjection {
	access_token: Zeroizing<String>,
	provider_account_id: String,
	plan_type: Option<String>,
}
impl ChatgptRefreshProjection {
	pub(crate) fn new(
		access_token: String,
		provider_account_id: String,
		plan_type: Option<String>,
	) -> Result<Self, CredentialVaultError> {
		if access_token.is_empty()
			|| provider_account_id.is_empty()
			|| provider_account_id.len() > 512
			|| provider_account_id.chars().any(char::is_control)
		{
			return Err(CredentialVaultError::ProjectionRejected);
		}
		Ok(Self { access_token: Zeroizing::new(access_token), provider_account_id, plan_type })
	}
}
impl Debug for ChatgptRefreshProjection {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ChatgptRefreshProjection")
			.field("access_token", &"[REDACTED]")
			.field("provider_account_id", &self.provider_account_id)
			.field("plan_type", &self.plan_type)
			.finish()
	}
}

/// Immutable shared-home account authority. There is intentionally no rebinding API.
///
/// ```compile_fail
/// use decodex_codex::AccountBinding;
/// use decodex_core::AccountId;
///
/// let first = AccountId::new("10000000-0000-4000-8000-000000000001")?;
/// let second = AccountId::new("10000000-0000-4000-8000-000000000002")?;
/// let mut binding = AccountBinding::shared_home(first)?;
/// binding.rebind(second);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub(crate) struct AccountBinding {
	account_id: AccountId,
	expected_codex_home: PathBuf,
	process_binding: Option<ProcessGenerationAccountBinding>,
	refresh_callback: Option<Arc<dyn AccountRefreshCallback>>,
}
impl AccountBinding {
	/// Bind one child to the account resolved from the child's immutable Codex home.
	pub fn shared_home(account_id: AccountId) -> Result<Self, SupervisionError> {
		let home = env::var_os("HOME")
			.filter(|home| !home.is_empty())
			.ok_or(SupervisionError::InvalidBinding)?;

		Ok(Self {
			account_id,
			expected_codex_home: PathBuf::from(home).join(".codex"),
			process_binding: None,
			refresh_callback: None,
		})
	}

	/// Bind an account-ready launch to exact registry/store/provider/callback facts.
	pub(crate) fn shared_home_bound(
		account_id: AccountId,
		process_binding: ProcessGenerationAccountBinding,
		refresh_callback: Arc<dyn AccountRefreshCallback>,
	) -> Result<Self, SupervisionError> {
		let mut binding = Self::shared_home(account_id)?;
		binding.process_binding = Some(process_binding);
		binding.refresh_callback = Some(refresh_callback);
		Ok(binding)
	}

	#[cfg(test)]
	fn for_test(expected_codex_home: PathBuf) -> Self {
		Self {
			account_id: AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			expected_codex_home,
			process_binding: None,
			refresh_callback: None,
		}
	}

	/// Construct one synthetic binding for cross-adapter contract tests.
	#[cfg(test)]
	#[doc(hidden)]
	pub fn fixture(account_id: AccountId, expected_codex_home: PathBuf) -> Self {
		Self { account_id, expected_codex_home, process_binding: None, refresh_callback: None }
	}

	/// Exact non-secret account selected before process creation.
	pub fn account_id(&self) -> &AccountId {
		&self.account_id
	}

	fn process_binding(&self) -> Result<&ProcessGenerationAccountBinding, SupervisionError> {
		self.process_binding.as_ref().ok_or(SupervisionError::InvalidBinding)
	}
}

impl Debug for AccountBinding {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AccountBinding").finish_non_exhaustive()
	}
}

/// Exact executable contract for one supervised app-server build.
#[derive(Clone)]
pub(super) struct AppServerCommand {
	program: PathBuf,
	executable: Arc<ExecutableSnapshot>,
	executable_digest: [u8; 32],
	#[cfg(target_os = "macos")]
	attested_code_identity: Option<AttestedCodeIdentity>,
	app_server_args: Vec<OsString>,
	version_args: Vec<OsString>,
	schema_args: Vec<OsString>,
	working_directory: PathBuf,
	#[cfg(test)]
	preflight_cleanup_test: Option<PreflightCleanupTest>,
	#[cfg(test)]
	before_spawn_test: Option<BeforeSpawnTest>,
	#[cfg(test)]
	after_verification_test: Option<BeforeSpawnTest>,
	#[cfg(all(test, target_os = "macos"))]
	test_spawn_path: Option<PathBuf>,
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
		let (program, executable, executable_digest) = resolve_executable(OsStr::new("codex"))?;
		let mut command = Self::production_from_resolved(
			program,
			executable,
			executable_digest,
			working_directory.into(),
		);

		#[cfg(target_os = "macos")]
		{
			command.attested_code_identity = Some(
				AttestedCodeIdentity::capture(
					&command.executable.execution_path(),
					&command.program,
				)
				.map_err(|_| SupervisionError::ExecutableUnavailable)?,
			);
		}

		Ok(command)
	}

	fn production_from_resolved(
		program: PathBuf,
		executable: Arc<ExecutableSnapshot>,
		executable_digest: [u8; 32],
		working_directory: PathBuf,
	) -> Self {
		Self {
			program,
			executable,
			executable_digest,
			#[cfg(target_os = "macos")]
			attested_code_identity: None,
			app_server_args: vec!["app-server".into(), "--stdio".into()],
			version_args: vec!["--version".into()],
			schema_args: vec![
				"app-server".into(),
				"generate-json-schema".into(),
				"--experimental".into(),
				"--out".into(),
			],
			working_directory,
			#[cfg(test)]
			preflight_cleanup_test: None,
			#[cfg(test)]
			before_spawn_test: None,
			#[cfg(test)]
			after_verification_test: None,
			#[cfg(all(test, target_os = "macos"))]
			test_spawn_path: None,
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
		let (program, executable, executable_digest) =
			resolve_executable(program.as_os_str()).expect("fake executable must resolve");

		Self {
			program,
			executable,
			executable_digest,
			#[cfg(target_os = "macos")]
			attested_code_identity: None,
			app_server_args: app_server_args.into_iter().map(Into::into).collect(),
			version_args: version_args.into_iter().map(Into::into).collect(),
			schema_args: schema_args.into_iter().map(Into::into).collect(),
			working_directory: working_directory.into(),
			preflight_cleanup_test: None,
			before_spawn_test: None,
			after_verification_test: None,
			#[cfg(target_os = "macos")]
			test_spawn_path: None,
		}
	}

	/// Construct the repository's synthetic app-server fixture command.
	#[cfg(test)]
	#[doc(hidden)]
	pub fn fixture(
		mode: &str,
		working_directory: impl Into<PathBuf>,
		extra: Option<&Path>,
	) -> Self {
		let fixture =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_app_server.py");
		let mut app_server_args =
			vec!["-B".into(), fixture.clone().into_os_string(), "serve".into(), mode.into()];

		if let Some(extra) = extra {
			app_server_args.push(extra.as_os_str().to_owned());
		}

		let (program, executable, executable_digest) = resolve_executable(OsStr::new("python3"))
			.expect("the synthetic app-server interpreter is available");

		Self {
			program,
			executable,
			executable_digest,
			#[cfg(target_os = "macos")]
			attested_code_identity: None,
			app_server_args,
			version_args: vec!["-B".into(), fixture.clone().into_os_string(), "--version".into()],
			schema_args: vec![
				"-B".into(),
				fixture.into_os_string(),
				"generate-json-schema".into(),
				"--out".into(),
			],
			working_directory: working_directory.into(),
			#[cfg(test)]
			preflight_cleanup_test: None,
			#[cfg(test)]
			before_spawn_test: None,
			#[cfg(test)]
			after_verification_test: None,
			#[cfg(all(test, target_os = "macos"))]
			test_spawn_path: None,
		}
	}

	#[cfg(all(test, target_os = "macos"))]
	fn with_spawn_path_for_test(mut self, path: PathBuf) -> Self {
		self.test_spawn_path = Some(path);

		self
	}

	#[cfg(test)]
	fn with_uncertain_preflight_for_test(
		mut self,
		trigger_spawn: u32,
		spawn_count: Arc<AtomicU32>,
		process_group: Arc<AtomicU32>,
		reaper_delay: Duration,
	) -> Self {
		self.preflight_cleanup_test = Some(PreflightCleanupTest {
			trigger_spawn,
			spawn_count,
			process_group,
			reaper_delay,
			quarantine: ProcessQuarantine::new(),
		});

		self
	}

	#[cfg(test)]
	fn with_preflight_cleanup_control_for_test(
		mut self,
		trigger_spawn: u32,
		spawn_count: Arc<AtomicU32>,
		process_group: Arc<AtomicU32>,
		reaper_delay: Duration,
		quarantine: Arc<ProcessQuarantine>,
	) -> Self {
		self.preflight_cleanup_test = Some(PreflightCleanupTest {
			trigger_spawn,
			spawn_count,
			process_group,
			reaper_delay,
			quarantine,
		});

		self
	}

	#[cfg(test)]
	fn with_before_spawn_for_test(
		mut self,
		trigger_spawn: u32,
		spawn_count: Arc<AtomicU32>,
		action: Arc<dyn Fn() + Send + Sync>,
	) -> Self {
		self.before_spawn_test = Some(BeforeSpawnTest { trigger_spawn, spawn_count, action });

		self
	}

	#[cfg(test)]
	fn with_after_verification_for_test(
		mut self,
		trigger_spawn: u32,
		spawn_count: Arc<AtomicU32>,
		action: Arc<dyn Fn() + Send + Sync>,
	) -> Self {
		self.after_verification_test = Some(BeforeSpawnTest { trigger_spawn, spawn_count, action });

		self
	}

	#[cfg(all(test, target_os = "macos"))]
	fn with_attested_spawn_for_test(mut self) -> Self {
		self.attested_code_identity = Some(
			AttestedCodeIdentity::capture(&self.executable.execution_path(), &self.program)
				.expect("the synthetic executable has a valid static code identity"),
		);

		self
	}
}

impl Debug for AppServerCommand {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AppServerCommand").finish_non_exhaustive()
	}
}

/// Daemon-lifetime observed Codex capability evidence shared by account-scoped launches.
///
/// Startup constructs this profile once from one immutable executable snapshot and one generated
/// schema preflight. Each account launch still verifies the canonical executable, binds the exact
/// account credential revision, and uses suspended dynamic code-identity attestation before user
/// code can run.
#[derive(Clone)]
pub(crate) struct AttestedAppServerProfile {
	command: AppServerCommand,
	build: BuildId,
	generated: GeneratedSchemaEvidence,
	capability: ExactBuildLaunchCapability,
}
impl AttestedAppServerProfile {
	/// Prove the current Codex executable and generated account callback contract at startup.
	pub(crate) fn attest(
		working_directory: impl Into<PathBuf>,
		timeout: Duration,
	) -> Result<Self, ProbeError> {
		let command = AppServerCommand::new(working_directory)?;
		validated_working_directory(&command)?;
		let capability = ExactBuildLaunchCapability::attest_profile(&command)?;
		let home = env::var_os("HOME")
			.filter(|home| !home.is_empty())
			.ok_or(SupervisionError::InvalidBinding)?;
		let expected_codex_home = PathBuf::from(home).join(".codex");
		let (build, generated, guard) =
			attest_executable_for_home(&command, &expected_codex_home, timeout, None)?;
		if guard.is_some() {
			return Err(SupervisionError::CleanupUnavailable.into());
		}
		Ok(Self { command, build, generated, capability })
	}

	/// Return the credential-negative callback evidence persisted by Account Service.
	pub(crate) fn account_callback_attestation(&self) -> CodexAccountCapabilityAttestation {
		CodexAccountCapabilityAttestation {
			build_identity: self.build.as_str().to_owned(),
			executable_sha256: hex_digest(&self.command.executable_digest),
			schema_sha256: self.generated.fingerprint.clone(),
			callback_profile_sha256: self.generated.account_callback_profile_sha256().to_owned(),
			login_chatgpt_auth_tokens: true,
			refresh_callback: true,
		}
	}

	#[cfg(test)]
	fn attest_for_test(
		command: AppServerCommand,
		expected_codex_home: &Path,
		timeout: Duration,
	) -> Result<Self, ProbeError> {
		validated_working_directory(&command)?;
		let (build, generated, guard) =
			attest_executable_for_home(&command, expected_codex_home, timeout, None)?;
		if guard.is_some() {
			return Err(SupervisionError::CleanupUnavailable.into());
		}

		Ok(Self {
			command,
			build,
			generated,
			capability: ExactBuildLaunchCapability::PrivateStdioDisabledEphemeralStartupV1,
		})
	}

	/// Bind one account and capacity permit without repeating immutable build preflights.
	fn bind(
		mut self,
		working_directory: PathBuf,
		binding: AccountBinding,
		timeout: Duration,
		guard: RunnerPermit,
	) -> Result<AttestedAppServerLaunch, ProbeError> {
		if guard.account_id.as_str() != binding.account_id.as_str() || guard.account_revision < 1 {
			return Err(SupervisionError::InvalidBinding.into());
		}
		self.command.working_directory = working_directory;
		validated_working_directory(&self.command)?;
		let process_binding = binding.process_binding()?;
		if self.generated.account_callback_profile_sha256()
			!= process_binding.refresh_callback_profile_sha256
		{
			return Err(SupervisionError::LaunchCapabilityUnavailable.into());
		}
		let runner_identity =
			attested_launch_identity(&self.command, &binding, &self.build, self.capability)?;

		Ok(AttestedAppServerLaunch {
			command: self.command,
			binding,
			build: self.build,
			generated: self.generated,
			runner_identity,
			capability: self.capability,
			timeout,
			guard,
			quick_task_pre_spawn_check: None,
		})
	}
}

fn validated_working_directory(command: &AppServerCommand) -> Result<String, ProbeError> {
	command
		.working_directory
		.to_str()
		.filter(|value| {
			value.starts_with('/')
				&& !value.is_empty()
				&& value.len() <= MAX_QUICK_TASK_WORKING_DIRECTORY_BYTES
				&& !value.chars().any(char::is_control)
				&& !decodex_core::contains_credential_material(value)
		})
		.map(str::to_owned)
		.ok_or_else(|| SupervisionError::InvalidBinding.into())
}

impl Debug for AttestedAppServerProfile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AttestedAppServerProfile").finish_non_exhaustive()
	}
}

/// Runtime launch capability derived from the current executable and protocol preflight.
///
/// This type has no public constructor. The fixed app-server command must match the process
/// contract. Codex consumes
/// `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` at process startup and selects
/// `DisabledEphemeral` when `app-server --stdio` has no remote-control argument. This marker is
/// startup-state evidence, not a permanent in-process policy. ProcessGeneration therefore keeps
/// the raw channels private and returns no protocol writer. Unsupported protocol shapes still
/// fail closed after runtime schema validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExactBuildLaunchCapability {
	PrivateStdioDisabledEphemeralStartupV1,
}
impl ExactBuildLaunchCapability {
	/// Reject unsupported platforms and process shapes before a profile-dependent preflight can
	/// spawn a child.
	fn attest_profile(command: &AppServerCommand) -> Result<Self, SupervisionError> {
		if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
			return Err(SupervisionError::LaunchCapabilityUnavailable);
		}
		let exact_args = command.app_server_args.len() == 2
			&& command.app_server_args[0].as_os_str() == OsStr::new("app-server")
			&& command.app_server_args[1].as_os_str() == OsStr::new("--stdio");
		if !exact_args {
			return Err(SupervisionError::LaunchCapabilityUnavailable);
		}

		Ok(Self::PrivateStdioDisabledEphemeralStartupV1)
	}

	const fn identity(self) -> &'static str {
		match self {
			Self::PrivateStdioDisabledEphemeralStartupV1 => PRIVATE_STDIO_CAPABILITY_ID,
		}
	}

	const fn lifetime(self) -> ExactProcessGenerationLifetimeCapability {
		match self {
			Self::PrivateStdioDisabledEphemeralStartupV1 =>
				ExactProcessGenerationLifetimeCapability::MacosPrivateStdioBestEffortEofV1,
		}
	}
}

/// Exact lifetime capability derived only from one accepted executable profile.
///
/// There is intentionally no Linux variant. Generic session and descriptor setup cannot install
/// `PR_SET_PDEATHSIG`; a future Linux primitive requires a separately accepted exact profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExactProcessGenerationLifetimeCapability {
	MacosPrivateStdioBestEffortEofV1,
}
impl ExactProcessGenerationLifetimeCapability {
	const fn control_kind(self) -> ProcessControlKind {
		match self {
			Self::MacosPrivateStdioBestEffortEofV1 => ProcessControlKind::StdioOnlyBestEffortEof,
		}
	}

	fn configure(self, _command: &mut Command) -> Result<(), SupervisionError> {
		match self {
			Self::MacosPrivateStdioBestEffortEofV1
				if cfg!(all(target_os = "macos", target_arch = "aarch64")) =>
				Ok(()),
			Self::MacosPrivateStdioBestEffortEofV1 =>
				Err(SupervisionError::LaunchCapabilityUnavailable),
		}
	}
}

/// One non-forgeable account-bound app-server launch.
///
/// It retains the protected executable snapshot and derives the durable launch-manifest identity
/// from the same verified executable, command, arguments, working directory, environment, account,
/// and runtime capability that it later spawns. No mutable [`Command`] or caller-supplied runner
/// digest crosses the ProcessSupervisor boundary.
pub(crate) struct AttestedAppServerLaunch {
	command: AppServerCommand,
	binding: AccountBinding,
	build: BuildId,
	generated: GeneratedSchemaEvidence,
	runner_identity: ProcessRunnerIdentity,
	capability: ExactBuildLaunchCapability,
	timeout: Duration,
	guard: RunnerPermit,
	quick_task_pre_spawn_check: Option<Arc<dyn QuickTaskPreSpawnCheck>>,
}

/// Final synchronous check owned by Quick Task and executed at the child creation boundary.
pub(crate) trait QuickTaskPreSpawnCheck: Send + Sync {
	fn validate_at_spawn_boundary(&self) -> Result<(), ()>;
	fn working_directory_descriptor(&self) -> i32;
}

impl AttestedAppServerLaunch {
	/// Bind one account launch to the daemon's already-attested immutable build profile.
	pub(crate) fn bind(
		profile: AttestedAppServerProfile,
		binding: AccountBinding,
		timeout: Duration,
		guard: RunnerPermit,
	) -> Result<Self, ProbeError> {
		let working_directory = profile.command.working_directory.clone();
		profile.bind(working_directory, binding, timeout, guard)
	}

	/// Bind one Quick Task launch to its command-selected working directory.
	pub(crate) fn bind_selected_working_directory(
		profile: AttestedAppServerProfile,
		working_directory: PathBuf,
		binding: AccountBinding,
		timeout: Duration,
		guard: RunnerPermit,
	) -> Result<Self, ProbeError> {
		profile.bind(working_directory, binding, timeout, guard)
	}

	/// Derive all durable pre-spawn facts that belong to the opaque launch authority.
	pub(crate) fn derive_intent(
		&self,
		generation_id: ProcessGenerationId,
		intended_boot_id: ProcessBootIdentity,
		execution_authorization: ProcessExecutionAuthorization,
	) -> ProcessGenerationIntent {
		ProcessGenerationIntent {
			generation_id,
			account_id: self.binding.account_id.clone(),
			runner_identity: self.runner_identity.clone(),
			intended_boot_id,
			control_kind: self.capability.lifetime().control_kind(),
			isolation_kind: ProcessIsolationKind::Session,
			execution_authorization,
		}
	}

	/// Return the immutable account binding covered by this launch manifest.
	pub(crate) fn account_binding(&self) -> &ProcessGenerationAccountBinding {
		self.binding
			.process_binding
			.as_ref()
			.expect("attested launch always has an account binding")
	}

	/// Spawn the canonical macOS image under snapshot-rooted dynamic attestation.
	///
	/// This method returns an error only before a child exists.
	pub(crate) fn spawn(self) -> Result<AttestedProcessChild, SupervisionError> {
		let Self {
			command,
			binding,
			build,
			generated,
			runner_identity: _,
			capability,
			timeout,
			guard,
			quick_task_pre_spawn_check,
		} = self;
		if ExactBuildLaunchCapability::attest_profile(&command)? != capability {
			return Err(SupervisionError::LaunchCapabilityUnavailable);
		}
		let process = SupervisedProcess::spawn_attested_with_pre_spawn_check(
			command,
			binding,
			guard,
			capability,
			quick_task_pre_spawn_check.as_deref(),
		)?;

		Ok(AttestedProcessChild { process, build, generated, timeout, initialized: false })
	}
}

/// Consume one fresh exact Quick Task admission before the ordinary supervisor may spawn.
pub(crate) async fn spawn_admitted_quick_task_process(
	control: &ProcessGenerationControl,
	admission: FreshQuickTaskProcessGeneration,
	execution_authorization: ProcessExecutionAuthorization,
	mut launch: AttestedAppServerLaunch,
	pre_spawn_check: Arc<dyn QuickTaskPreSpawnCheck>,
) -> Result<FencedProcess, ProcessSupervisorError> {
	launch.quick_task_pre_spawn_check = Some(pre_spawn_check);
	control.spawn_fenced_quick_task(admission, execution_authorization, launch).await
}

/// Exact newly spawned protocol child plus immutable build evidence and capacity authority.
pub(crate) struct AttestedProcessChild {
	process: SupervisedProcess,
	build: BuildId,
	generated: GeneratedSchemaEvidence,
	timeout: Duration,
	initialized: bool,
}
impl AttestedProcessChild {
	pub(crate) fn process_id(&self) -> u32 {
		self.process.process_id()
	}

	pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
		self.process.owner.try_wait()
	}

	pub(crate) fn may_signal_process_group(&self) -> bool {
		self.process.owner.may_signal_process_group()
	}

	pub(crate) fn has_private_lifetime_channels(&self) -> bool {
		self.process.owner.child.is_some() && self.process.owner.pump.is_some()
	}

	/// Close private lifetime channels without returning either raw protocol handle.
	pub(crate) fn close_private_lifetime_channels(&mut self) {
		self.process.stdin = Box::new(io::sink());
	}

	/// Initialize and project one exact credential only after ProcessGeneration identity binding.
	pub(super) fn initialize_reset_card(
		&mut self,
		vault: &dyn CredentialVault,
	) -> Result<(), ResetCardProcessError> {
		if self.initialized {
			return Err(ResetCardProcessError::ProcessUnavailable);
		}
		let reset_profile = ResetCardCapabilityProfile::from_schema(self.generated.contract());
		if !reset_profile.is_supported() {
			return Err(ResetCardProcessError::SchemaUnsupported(reset_profile.state()));
		}
		let mut cache = CapabilityCache::default();
		let mut negotiation = ProbeNegotiation::new(&mut cache, &self.build, &self.generated);
		initialize_probe(&mut self.process, Some(vault), self.timeout, &mut negotiation)
			.map_err(ResetCardProcessError::from_probe)?;
		self.initialized = true;
		Ok(())
	}

	/// Initialize one exact account-bound child for ordinary Conversation I/O.
	pub(crate) fn initialize_ordinary_turns(
		&mut self,
		vault: &dyn CredentialVault,
	) -> Result<(), QuickTaskProcessError> {
		if self.initialized {
			return Err(QuickTaskProcessError::Unavailable);
		}
		self.generated
			.contract()
			.check_quick_task_contract()
			.map_err(|_| QuickTaskProcessError::Incompatible)?;
		let mut cache = CapabilityCache::default();
		let mut negotiation = ProbeNegotiation::new(&mut cache, &self.build, &self.generated);
		initialize_probe(&mut self.process, Some(vault), self.timeout, &mut negotiation)
			.map_err(|_| QuickTaskProcessError::Unavailable)?;
		self.initialized = true;
		Ok(())
	}

	/// Reserve the exact `thread/start` frame before its PostgreSQL fence is committed.
	pub(crate) fn prepare_ordinary_thread_start(
		&mut self,
		request: &QuickTaskThreadStartRequest,
	) -> Result<PreparedThreadStart, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		Ok(PreparedThreadStart {
			request: request.clone(),
			wire: self.process.prepare_quick_task_request("thread/start", request)?,
		})
	}

	/// Consume the only fresh RuntimeSession fence and send its exact `thread/start` once.
	pub(crate) fn start_ordinary_thread(
		&mut self,
		prepared: PreparedThreadStart,
		authority: FreshRuntimeSessionThreadStart,
	) -> Result<EstablishedOrdinaryThread, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		let PreparedThreadStart { request, wire } = prepared;
		let readback = authority.readback();
		if readback.thread_start_request_id != wire.request_id
			|| readback.thread_start_request_sha256.as_str() != wire.request_sha256.as_str()
		{
			return Err(QuickTaskProcessError::Incompatible);
		}
		let request_id = wire.request_id;
		let request_sha256 = wire.request_sha256.clone();
		let success = self.process.quick_task_request(wire, self.timeout, true, |bytes| {
			decode_quick_task_thread_start_response(&request, bytes)
		})?;
		let codex_thread_id = success.value.thread_id().as_str().to_owned();
		let binding = authority.into_binding(SuccessfulRuntimeSessionThreadStart {
			response_id: success.wire.response_id,
			response_sha256: success.wire.response_sha256,
			codex_thread_id: codex_thread_id.clone(),
		});
		debug_assert_eq!(request_id, binding.thread_start_request_id);
		debug_assert_eq!(request_sha256, binding.thread_start_request_sha256);
		Ok(EstablishedOrdinaryThread { codex_thread_id, binding, events: success.events })
	}

	/// Resume one exact normal Codex thread and return exact positive wire facts.
	pub(crate) fn resume_ordinary_thread(
		&mut self,
		request: &QuickTaskThreadResumeRequest,
	) -> Result<ResumedOrdinaryThread, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		let wire = self.process.prepare_quick_task_request("thread/resume", request)?;
		let success = self.process.quick_task_request(wire, self.timeout, false, |bytes| {
			decode_quick_task_thread_resume_response(request, bytes)
		})?;
		Ok(ResumedOrdinaryThread {
			codex_thread_id: success.value.thread_id().as_str().to_owned(),
			request_id: success.wire.request_id,
			request_sha256: success.wire.request_sha256,
			response_id: success.wire.response_id,
			response_sha256: success.wire.response_sha256,
			events: success.events,
		})
	}

	/// Reserve the exact `turn/start` frame before ProviderAttempt preparation.
	pub(crate) fn prepare_ordinary_turn_start(
		&mut self,
		attempt_id: ProviderAttemptId,
		request: &QuickTaskTurnStartRequest,
	) -> Result<PreparedTurnStart, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		Ok(PreparedTurnStart {
			attempt_id,
			wire: self.process.prepare_quick_task_request("turn/start", request)?,
		})
	}

	/// Consume one fresh generic dispatch fence and send its exact `turn/start` once.
	pub(crate) fn start_ordinary_turn(
		&mut self,
		prepared: PreparedTurnStart,
		authority: FreshProviderDispatchFence,
	) -> Result<StartedOrdinaryTurn, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		let PreparedTurnStart { attempt_id, wire } = prepared;
		if authority.attempt_id() != &attempt_id {
			return Err(QuickTaskProcessError::Incompatible);
		}
		let success = self.process.quick_task_request(
			wire,
			self.timeout,
			true,
			decode_quick_task_turn_start_response,
		)?;
		Ok(StartedOrdinaryTurn {
			turn_id: success.value.turn_id().as_str().to_owned(),
			status: success.value.status(),
			response_sha256: success.wire.response_sha256,
			events: success.events,
		})
	}

	/// Poll one bounded ordinary notification while retaining exact child ownership.
	pub(crate) fn next_ordinary_turn_event(
		&mut self,
		wait: Duration,
	) -> Result<Option<QuickTaskProcessEvent>, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		self.process.next_quick_task_event(wait)
	}

	/// Interrupt one exact active turn. Terminal state still comes from a typed notification.
	pub(crate) fn interrupt_ordinary_turn(
		&mut self,
		request: &QuickTaskTurnInterruptRequest,
	) -> Result<Vec<QuickTaskProcessEvent>, QuickTaskProcessError> {
		self.require_ordinary_turns_initialized()?;
		let wire = self.process.prepare_quick_task_request("turn/interrupt", request)?;
		self.process
			.quick_task_request(wire, self.timeout, true, decode_quick_task_turn_interrupt_response)
			.map(|success: QuickTaskProcessSuccess<QuickTaskTurnInterruptResponse>| success.events)
	}

	fn require_ordinary_turns_initialized(&self) -> Result<(), QuickTaskProcessError> {
		self.initialized.then_some(()).ok_or(QuickTaskProcessError::Unavailable)
	}

	/// Initialize one fresh callback-capability child with the synthetic probe credential.
	///
	/// Cloud-config's unauthorized response during the synthetic initial login triggers the refresh
	/// callback. The later inventory read and account readback only prove the installed real
	/// successor.
	pub(super) fn initialize_callback_probe(
		&mut self,
		vault: &dyn CredentialVault,
	) -> Result<(), ResetCardProcessError> {
		if self.initialized {
			return Err(ResetCardProcessError::ProcessUnavailable);
		}
		let reset_profile = ResetCardCapabilityProfile::from_schema(self.generated.contract());
		if !reset_profile.is_supported() {
			return Err(ResetCardProcessError::SchemaUnsupported(reset_profile.state()));
		}
		let mut cache = CapabilityCache::default();
		let mut negotiation = ProbeNegotiation::new(&mut cache, &self.build, &self.generated);
		initialize_probe_projection(&mut self.process, vault, self.timeout, &mut negotiation)
			.map_err(ResetCardProcessError::from_probe)?;
		self.initialized = true;
		Ok(())
	}

	/// Read one bounded inventory and re-attest the immutable account before returning it.
	pub(super) fn read_reset_card_inventory(
		&mut self,
	) -> Result<ResetCardInventory, ResetCardProcessError> {
		if !self.initialized {
			return Err(ResetCardProcessError::ProcessUnavailable);
		}
		let inventory = read_reset_card_inventory(&mut self.process, self.timeout)?;
		re_attest_reset_card_account(&mut self.process, self.timeout)?;
		Ok(inventory)
	}

	/// Prove the unauthorized refresh callback and exact successor readback path once.
	pub(super) fn prove_refresh_callback(&mut self) -> Result<(), ResetCardProcessError> {
		if !self.initialized {
			return Err(ResetCardProcessError::ProcessUnavailable);
		}
		let _ = read_reset_card_inventory(&mut self.process, self.timeout)?;
		re_attest_reset_card_account(&mut self.process, self.timeout)
	}

	/// Consume one fenced credit, read back inventory, and re-attest the account.
	pub(super) fn consume_reset_card(
		&mut self,
		credit_id: ExactResetCreditId,
		idempotency_key: ResetCardIdempotencyKey,
	) -> Result<ResetCardConsumeReadback, ResetCardProcessError> {
		if !self.initialized {
			return Err(ResetCardProcessError::ProcessUnavailable);
		}
		let outcome =
			consume_reset_card(&mut self.process, self.timeout, credit_id, idempotency_key)?;
		let inventory = read_reset_card_inventory(&mut self.process, self.timeout)?;
		re_attest_reset_card_account(&mut self.process, self.timeout)?;
		Ok(ResetCardConsumeReadback { outcome, inventory })
	}
}

/// Exact request facts reserved before a RuntimeSession thread-start fence.
pub(crate) struct PreparedThreadStart {
	request: QuickTaskThreadStartRequest,
	wire: PreparedQuickTaskRequest,
}
impl PreparedThreadStart {
	pub(crate) const fn request_id(&self) -> i64 {
		self.wire.request_id
	}

	pub(crate) fn request_sha256(&self) -> &str {
		&self.wire.request_sha256
	}
}

/// Successful durable thread establishment ready for the exact bind command.
pub(crate) struct EstablishedOrdinaryThread {
	pub(crate) codex_thread_id: String,
	pub(crate) binding: BindRuntimeSessionThread,
	pub(crate) events: Vec<QuickTaskProcessEvent>,
}

/// Successful exact-thread resume facts ready for one affine runtime proof.
pub(crate) struct ResumedOrdinaryThread {
	pub(crate) codex_thread_id: String,
	pub(crate) request_id: i64,
	pub(crate) request_sha256: String,
	pub(crate) response_id: i64,
	pub(crate) response_sha256: String,
	pub(crate) events: Vec<QuickTaskProcessEvent>,
}

/// Exact `turn/start` request reserved before generic ProviderAttempt preparation.
pub(crate) struct PreparedTurnStart {
	attempt_id: ProviderAttemptId,
	wire: PreparedQuickTaskRequest,
}
impl PreparedTurnStart {
	pub(crate) const fn request_id(&self) -> i64 {
		self.wire.request_id
	}

	pub(crate) fn request_sha256(&self) -> &str {
		&self.wire.request_sha256
	}
}

/// Successful `turn/start` response plus notifications observed before that response.
pub(crate) struct StartedOrdinaryTurn {
	pub(crate) turn_id: String,
	pub(crate) status: decodex_codex::QuickTaskTurnStatus,
	pub(crate) response_sha256: String,
	pub(crate) events: Vec<QuickTaskProcessEvent>,
}

/// Closed user-visible event set emitted by the private ordinary-turn child gateway.
pub(crate) enum QuickTaskProcessEvent {
	/// One bounded assistant-message delta.
	MessageDelta(QuickTaskMessageDelta),
	/// One exact turn reached a terminal app-server notification.
	TurnCompleted {
		/// Opaque exact provider turn identity.
		turn_id: String,
		/// Closed terminal provider state.
		status: TurnStatus,
		/// SHA-256 witness of the accepted notification frame.
		witness_digest: String,
	},
}

/// Closed private-gateway failure that never embeds provider or credential text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskProcessError {
	/// No method bytes were admitted because initialization or local authority was unavailable.
	Unavailable,
	/// The exact app-server method rejected the request with a bounded response witness.
	Rejected { witness_digest: String },
	/// App-server bytes contradicted the accepted typed contract.
	Incompatible,
	/// Local supervision was lost while a blocking operation may already have crossed send.
	ControlLost,
	/// Request bytes may have reached app-server and no usable exact response was retained.
	Ambiguous { request_id: i64, request_sha256: String },
}

struct PreparedQuickTaskRequest {
	request_id: i64,
	request_sha256: String,
	frame: ZeroizingOutboundFrame,
}

struct QuickTaskWireReceipt {
	request_id: i64,
	request_sha256: String,
	response_id: i64,
	response_sha256: String,
}

struct QuickTaskProcessSuccess<T> {
	value: T,
	wire: QuickTaskWireReceipt,
	events: Vec<QuickTaskProcessEvent>,
}

fn callback_probe_access_token(
	provider_account_id: &str,
) -> Result<Zeroizing<String>, ResetCardProcessError> {
	// This observed executable refreshes external auth after a read-only cloud-config 401, but not
	// after `account/rateLimits/read`. A business-shaped, invalidly signed JWT reaches that path
	// without creating a thread or turn. The separate RPC account ID and this claim stay bound to
	// the immutable ProcessGeneration provider; the Account Service supplies the real successor
	// token. Omitting the user ID makes the cloud-config cache identity incomplete and forces a
	// read. After a successful callback, Codex writes any resulting cache under the refreshed real
	// identity.
	let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
	let payload = serde_json::to_vec(&serde_json::json!({
		"email": CALLBACK_PROBE_EMAIL,
		"https://api.openai.com/auth": {
			"chatgpt_plan_type": CALLBACK_PROBE_PLAN_TYPE,
			"chatgpt_account_id": provider_account_id,
		},
	}))
	.map_err(|_| ResetCardProcessError::InvalidProviderResponse)?;
	let payload = URL_SAFE_NO_PAD.encode(payload);
	let signature = URL_SAFE_NO_PAD.encode(CALLBACK_PROBE_SIGNATURE);

	Ok(Zeroizing::new(format!("{header}.{payload}.{signature}")))
}

/// Exact active-account identity retained only in zeroizing, redacted process memory.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct AccountIdentity {
	kind: Zeroizing<String>,
	email: Option<Zeroizing<String>>,
	requires_openai_auth: bool,
}
impl AccountIdentity {
	pub(crate) fn from_observation(
		kind: &str,
		email: Option<&str>,
		requires_openai_auth: bool,
	) -> Self {
		Self {
			kind: Zeroizing::new(kind.to_owned()),
			email: email.map(|email| Zeroizing::new(email.to_owned())),
			requires_openai_auth,
		}
	}
}
impl Debug for AccountIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AccountIdentity").finish_non_exhaustive()
	}
}

/// Default host-vault boundary until an operator supplies a concrete local vault.
/// It never reads ambient credentials and always keeps runner creation unavailable.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct UnavailableCredentialVault;
impl CredentialVault for UnavailableCredentialVault {
	fn project(
		&self,
		_account_id: &AccountId,
		_projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		Err(CredentialVaultError::Unavailable)
	}
}

/// Single-use process-scoped credential sink owned by the Codex adapter.
pub(crate) struct CredentialProjection<'a> {
	process: &'a mut SupervisedProcess,
	timeout: Duration,
	used: bool,
}
impl CredentialProjection<'_> {
	/// Authenticate this child with ChatGPT tokens held by the host vault.
	pub fn authenticate_chatgpt(
		&mut self,
		access_token: &str,
		provider_account_id: &str,
		plan_type: Option<&str>,
	) -> Result<(), CredentialVaultError> {
		if self.used {
			return Err(CredentialVaultError::ProjectionAlreadyUsed);
		}

		self.used = true;

		self.process
			.request::<_, CredentialProjectionResponse>(
				ReadOnlyMethod::AccountLoginStart,
				&ChatgptAuthParams {
					kind: "chatgptAuthTokens",
					access_token,
					chatgpt_account_id: provider_account_id,
					chatgpt_plan_type: plan_type,
				},
				self.timeout,
			)
			.map(|_| ())
			.map_err(|_| CredentialVaultError::ProjectionRejected)
	}

	/// Authenticate one fresh callback-capability child with the non-secret synthetic probe JWT.
	pub fn authenticate_callback_probe(
		&mut self,
		provider_account_id: &str,
	) -> Result<(), CredentialVaultError> {
		let access_token = callback_probe_access_token(provider_account_id)
			.map_err(|_| CredentialVaultError::ProjectionRejected)?;
		self.authenticate_chatgpt(
			access_token.as_str(),
			provider_account_id,
			Some(CALLBACK_PROBE_PLAN_TYPE),
		)
	}
}

impl Debug for CredentialProjection<'_> {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("CredentialProjection").finish_non_exhaustive()
	}
}

pub(super) struct StdoutPump {
	cancelled: Arc<AtomicBool>,
	done: Receiver<()>,
	thread: Option<JoinHandle<()>>,
}
impl StdoutPump {
	fn start<R>(
		reader: R,
		sender: SyncSender<InboundFrame>,
		protocol_limit_exceeded: Arc<AtomicBool>,
	) -> Result<Self, SupervisionError>
	where
		R: Read + std::os::fd::AsRawFd + Send + 'static,
	{
		Self::start_inner(reader, sender, protocol_limit_exceeded, None)
	}

	#[cfg(test)]
	fn start_with_buffer_barrier<R>(
		reader: R,
		sender: SyncSender<InboundFrame>,
		protocol_limit_exceeded: Arc<AtomicBool>,
		buffered: Arc<AtomicBool>,
	) -> Result<Self, SupervisionError>
	where
		R: Read + std::os::fd::AsRawFd + Send + 'static,
	{
		Self::start_inner(reader, sender, protocol_limit_exceeded, Some(buffered))
	}

	fn start_inner<R>(
		reader: R,
		sender: SyncSender<InboundFrame>,
		protocol_limit_exceeded: Arc<AtomicBool>,
		buffered: Option<Arc<AtomicBool>>,
	) -> Result<Self, SupervisionError>
	where
		R: Read + std::os::fd::AsRawFd + Send + 'static,
	{
		set_nonblocking(reader.as_raw_fd())?;

		let cancelled = Arc::new(AtomicBool::new(false));
		let pump_cancelled = Arc::clone(&cancelled);
		let (done_sender, done) = mpsc::sync_channel(1);
		let thread = Builder::new()
			.name("decodex-runtime-app-server-stdout".into())
			.spawn(move || {
				pump_stdout(
					reader,
					sender,
					protocol_limit_exceeded,
					&pump_cancelled,
					buffered.as_deref(),
				);

				let _ = done_sender.try_send(());
			})
			.map_err(|_| SupervisionError::SpawnFailed)?;

		Ok(Self { cancelled, done, thread: Some(thread) })
	}

	fn stop(&mut self, timeout: Duration) -> bool {
		let deadline = Instant::now() + timeout;

		self.cancelled.store(true, Ordering::Release);

		if let Some(thread) = &self.thread {
			thread.thread().unpark();
		}

		let stopped = if timeout.is_zero() {
			matches!(self.done.try_recv(), Ok(()) | Err(TryRecvError::Disconnected))
		} else {
			matches!(self.done.recv_timeout(timeout), Ok(()) | Err(RecvTimeoutError::Disconnected))
		};

		if !stopped {
			return false;
		}

		let Some(thread) = self.thread.as_ref() else {
			return true;
		};

		while !thread.is_finished() {
			if Instant::now() >= deadline {
				return false;
			}

			thread::yield_now();
		}

		self.thread.take().is_some_and(|thread| thread.join().is_ok())
	}
}

/// Owned app-server child and its immutable account authority.
pub(super) struct SupervisedProcess {
	owner: ProcessGroupOwner,
	stdin: Box<dyn Write + Send>,
	stdout: Receiver<InboundFrame>,
	protocol_limit_exceeded: Arc<AtomicBool>,
	binding: AccountBinding,
	#[cfg(test)]
	command: AppServerCommand,
	expected_account_identity: Option<AccountIdentity>,
	next_request_id: u64,
	abandoned_request_ids: BTreeSet<u64>,
}
impl SupervisedProcess {
	#[cfg(test)]
	fn spawn(command: AppServerCommand, binding: AccountBinding) -> Result<Self, SupervisionError> {
		let guard = RunnerCapacity::try_with_limit(1)
			.map_err(|_| SupervisionError::CleanupUnavailable)?
			.reserve(binding.account_id.clone(), 1)
			.map_err(|_| SupervisionError::SpawnFailed)?;

		Self::spawn_inner(command, binding, Some(guard))
	}

	fn spawn_bound(
		command: AppServerCommand,
		binding: AccountBinding,
		guard: RunnerPermit,
	) -> Result<Self, SupervisionError> {
		Self::spawn_inner(command, binding, Some(guard))
	}

	#[cfg(test)]
	fn spawn_attested(
		command: AppServerCommand,
		binding: AccountBinding,
		guard: RunnerPermit,
		capability: ExactBuildLaunchCapability,
	) -> Result<Self, SupervisionError> {
		Self::spawn_attested_with_pre_spawn_check(command, binding, guard, capability, None)
	}

	fn spawn_attested_with_pre_spawn_check(
		command: AppServerCommand,
		binding: AccountBinding,
		guard: RunnerPermit,
		capability: ExactBuildLaunchCapability,
		pre_spawn_check: Option<&dyn QuickTaskPreSpawnCheck>,
	) -> Result<Self, SupervisionError> {
		verify_canonical_executable_identity(&command)?;
		run_before_spawn_test(&command);
		verify_canonical_executable_identity(&command)?;
		run_after_verification_test(&command);
		let (sender, receiver) = mpsc::sync_channel(PROTOCOL_QUEUE_CAPACITY);
		let protocol_limit_exceeded = Arc::new(AtomicBool::new(false));
		let reader_limit_exceeded = Arc::clone(&protocol_limit_exceeded);
		let (owner, stdin) = spawn_attested_protocol_process(
			&command,
			&binding,
			guard,
			capability,
			pre_spawn_check,
			sender,
			reader_limit_exceeded,
		)?;

		Ok(Self {
			owner,
			stdin,
			stdout: receiver,
			protocol_limit_exceeded,
			binding,
			#[cfg(test)]
			command,
			expected_account_identity: None,
			next_request_id: 1,
			abandoned_request_ids: BTreeSet::new(),
		})
	}

	fn spawn_inner(
		command: AppServerCommand,
		binding: AccountBinding,
		guard: Option<RunnerPermit>,
	) -> Result<Self, SupervisionError> {
		verify_executable(&command)?;
		run_before_spawn_test(&command);
		verify_executable(&command)?;
		run_after_verification_test(&command);
		let (sender, receiver) = mpsc::sync_channel(PROTOCOL_QUEUE_CAPACITY);
		let protocol_limit_exceeded = Arc::new(AtomicBool::new(false));
		let reader_limit_exceeded = Arc::clone(&protocol_limit_exceeded);
		let (owner, stdin) =
			spawn_protocol_process(&command, &binding, guard, sender, reader_limit_exceeded)?;

		Ok(Self {
			owner,
			stdin,
			stdout: receiver,
			protocol_limit_exceeded,
			binding,
			#[cfg(test)]
			command,
			expected_account_identity: None,
			next_request_id: 1,
			abandoned_request_ids: BTreeSet::new(),
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

	/// Test-only restart proof for the immutable account binding and exact command/build.
	#[cfg(test)]
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
		self.request_rpc(method.as_str(), params, timeout).map_err(|error| match error {
			RpcError::Supervision(error) => ProbeError::Supervision(error),
			RpcError::MethodRejected(code) => ProbeError::MethodRejected { method, code },
		})
	}

	fn request_rpc<P, R>(
		&mut self,
		method: &'static str,
		params: &P,
		timeout: Duration,
	) -> Result<R, RpcError>
	where
		P: Serialize,
		R: DeserializeOwned,
	{
		let request_id = self.next_request_id;

		self.request_rpc_with_id(request_id, method, params, timeout).map(|success| success.value)
	}

	fn request_rpc_with_id<P, R>(
		&mut self,
		request_id: u64,
		method: &'static str,
		params: &P,
		timeout: Duration,
	) -> Result<RpcSuccess<R>, RpcError>
	where
		P: Serialize,
		R: DeserializeOwned,
	{
		if request_id < self.next_request_id || self.abandoned_request_ids.contains(&request_id) {
			return Err(RpcError::Supervision(SupervisionError::InvalidProtocol));
		}
		self.next_request_id = request_id
			.checked_add(1)
			.ok_or(RpcError::Supervision(SupervisionError::ProtocolLimitExceeded))?;
		let frame = exact_request_frame(request_id, method, params).map_err(rpc_supervision)?;
		let request_digest = frame.sha256();

		frame.write_to(&mut self.stdin).map_err(rpc_supervision)?;
		self.stdin.flush().map_err(|_| RpcError::Supervision(SupervisionError::WriteFailed))?;

		let deadline = Instant::now() + timeout;

		loop {
			let remaining = deadline.saturating_duration_since(Instant::now());

			if remaining.is_zero() {
				self.abandon_request(request_id)?;

				return Err(RpcError::Supervision(SupervisionError::ResponseTimeout));
			}

			let line = match self.stdout.recv_timeout(remaining) {
				Ok(line) => line.into_contiguous(),
				Err(RecvTimeoutError::Timeout) => {
					self.abandon_request(request_id)?;

					return Err(RpcError::Supervision(SupervisionError::ResponseTimeout));
				},
				Err(RecvTimeoutError::Disconnected) => {
					let error = if self.protocol_limit_exceeded.load(Ordering::Acquire) {
						SupervisionError::ProtocolLimitExceeded
					} else {
						SupervisionError::ProcessExited
					};

					self.abandon_request(request_id)?;

					return Err(RpcError::Supervision(error));
				},
			};

			Self::validate_zero_scratch_json(&line)
				.map_err(|()| RpcError::Supervision(SupervisionError::InvalidProtocol))?;

			let header: InboundHeader = serde_json::from_slice(&line)
				.map_err(|_| RpcError::Supervision(SupervisionError::InvalidProtocol))?;

			if let (Some(id), Some(method)) = (header.id, header.method.as_deref()) {
				self.service_inbound_request(id, method, &line).map_err(rpc_supervision)?;
				continue;
			}

			if header.id == Some(request_id) {
				let response_digest = hex_digest(&Sha256::digest(&line));
				let response: JsonRpcResponse<R> = serde_json::from_slice(&line)
					.map_err(|_| RpcError::Supervision(SupervisionError::InvalidProtocol))?;

				if response.id != request_id {
					return Err(RpcError::Supervision(SupervisionError::InvalidProtocol));
				}
				if !response.has_compatible_version() {
					return Err(RpcError::Supervision(SupervisionError::InvalidProtocol));
				}

				return match (response.result, response.error) {
					(Some(result), None) => Ok(RpcSuccess {
						value: result,
						wire: RpcWireReceipt {
							request_id: i64::try_from(request_id).map_err(|_| {
								RpcError::Supervision(SupervisionError::ProtocolLimitExceeded)
							})?,
							request_digest,
							response_id: i64::try_from(response.id).map_err(|_| {
								RpcError::Supervision(SupervisionError::ProtocolLimitExceeded)
							})?,
							response_digest,
						},
					}),
					(None, Some(error)) => Err(RpcError::MethodRejected(error.code)),
					_ => Err(RpcError::Supervision(SupervisionError::InvalidProtocol)),
				};
			}

			if let Some(id) = header.id {
				if self.abandoned_request_ids.remove(&id) {
					continue;
				}
				if header.method.is_none() {
					return Err(RpcError::Supervision(SupervisionError::InvalidProtocol));
				}
			}
		}
	}

	fn prepare_quick_task_request<P>(
		&mut self,
		method: &'static str,
		params: &P,
	) -> Result<PreparedQuickTaskRequest, QuickTaskProcessError>
	where
		P: Serialize,
	{
		let request_id = self.next_request_id;
		if self.abandoned_request_ids.contains(&request_id) {
			return Err(QuickTaskProcessError::Incompatible);
		}
		self.next_request_id =
			request_id.checked_add(1).ok_or(QuickTaskProcessError::Incompatible)?;
		let frame = exact_request_frame(request_id, method, params)
			.map_err(|_| QuickTaskProcessError::Incompatible)?;
		let request_id =
			i64::try_from(request_id).map_err(|_| QuickTaskProcessError::Incompatible)?;
		let request_sha256 = frame.sha256();
		Ok(PreparedQuickTaskRequest { request_id, request_sha256, frame })
	}

	fn quick_task_request<R>(
		&mut self,
		prepared: PreparedQuickTaskRequest,
		timeout: Duration,
		invalid_response_is_ambiguous: bool,
		decode: impl FnOnce(&[u8]) -> Result<R, decodex_codex::QuickTaskContractError>,
	) -> Result<QuickTaskProcessSuccess<R>, QuickTaskProcessError> {
		let PreparedQuickTaskRequest { request_id, request_sha256, frame } = prepared;
		let request_id_u64 =
			u64::try_from(request_id).map_err(|_| QuickTaskProcessError::Incompatible)?;
		let invalid_response = || {
			if invalid_response_is_ambiguous {
				QuickTaskProcessError::Ambiguous {
					request_id,
					request_sha256: request_sha256.clone(),
				}
			} else {
				QuickTaskProcessError::Incompatible
			}
		};
		let ambiguous = || QuickTaskProcessError::Ambiguous {
			request_id,
			request_sha256: request_sha256.clone(),
		};
		frame.write_to(&mut self.stdin).map_err(|_| ambiguous())?;
		self.stdin.flush().map_err(|_| ambiguous())?;

		let deadline = Instant::now() + timeout;
		let mut events = Vec::new();
		loop {
			let remaining = deadline.saturating_duration_since(Instant::now());
			if remaining.is_zero() {
				let _ = self.abandon_request(request_id_u64);
				return Err(ambiguous());
			}
			let line = match self.receive_quick_task_frame(remaining) {
				Ok(Some(line)) => line,
				Ok(None) | Err(_) => {
					let _ = self.abandon_request(request_id_u64);
					return Err(ambiguous());
				},
			};
			let header: InboundHeader =
				serde_json::from_slice(&line).map_err(|_| invalid_response())?;
			if let (Some(id), Some(inbound_method)) = (header.id, header.method.as_deref()) {
				self.service_inbound_request(id, inbound_method, &line).map_err(|_| ambiguous())?;
				continue;
			}
			if header.id == Some(request_id_u64) {
				let witness_digest = hex_digest(&Sha256::digest(&line));
				let response: JsonRpcResponse<serde_json::Value> =
					serde_json::from_slice(&line).map_err(|_| invalid_response())?;
				if response.id != request_id_u64 || !response.has_compatible_version() {
					return Err(invalid_response());
				}
				return match (response.result, response.error) {
					(Some(result), None) => {
						let bytes = serde_json::to_vec(&result).map_err(|_| invalid_response())?;
						let value = decode(&bytes).map_err(|_| invalid_response())?;
						Ok(QuickTaskProcessSuccess {
							value,
							wire: QuickTaskWireReceipt {
								request_id,
								request_sha256: request_sha256.clone(),
								response_id: i64::try_from(response.id)
									.map_err(|_| invalid_response())?,
								response_sha256: witness_digest,
							},
							events,
						})
					},
					(None, Some(_)) => Err(QuickTaskProcessError::Rejected { witness_digest }),
					_ => Err(invalid_response()),
				};
			}
			if header.id.is_none() {
				let event =
					decode_quick_task_process_event(&line).map_err(|_| invalid_response())?;
				let Some(event) = event else {
					continue;
				};
				if events.len() >= MAX_QUICK_TASK_BUFFERED_EVENTS {
					return Err(invalid_response());
				}
				events.push(event);
				continue;
			}
			if let Some(id) = header.id {
				if self.abandoned_request_ids.remove(&id) {
					continue;
				}
				return Err(invalid_response());
			}
		}
	}

	fn next_quick_task_event(
		&mut self,
		wait: Duration,
	) -> Result<Option<QuickTaskProcessEvent>, QuickTaskProcessError> {
		let line = match self.stdout.recv_timeout(wait) {
			Ok(line) => line.into_contiguous(),
			Err(RecvTimeoutError::Timeout) => return Ok(None),
			Err(RecvTimeoutError::Disconnected) => return Err(QuickTaskProcessError::Unavailable),
		};
		// Quick Task message notifications can contain ordinary JSON escapes. The frame remains
		// zeroizing and mechanically bounded; the landed typed contract validates its projection.
		let header: InboundHeader =
			serde_json::from_slice(&line).map_err(|_| QuickTaskProcessError::Incompatible)?;
		if let (Some(id), Some(method)) = (header.id, header.method.as_deref()) {
			self.service_inbound_request(id, method, &line)
				.map_err(|_| QuickTaskProcessError::Unavailable)?;
			return Ok(None);
		}
		if header.id.is_some() {
			return Err(QuickTaskProcessError::Incompatible);
		}
		decode_quick_task_process_event(&line)
	}

	fn receive_quick_task_frame(
		&mut self,
		wait: Duration,
	) -> Result<Option<Zeroizing<Vec<u8>>>, QuickTaskProcessError> {
		let line = match self.stdout.recv_timeout(wait) {
			Ok(line) => line.into_contiguous(),
			Err(RecvTimeoutError::Timeout) => return Ok(None),
			Err(RecvTimeoutError::Disconnected) => return Err(QuickTaskProcessError::Unavailable),
		};
		Ok(Some(line))
	}

	fn service_inbound_request(
		&mut self,
		id: u64,
		method: &str,
		line: &[u8],
	) -> Result<(), ProbeError> {
		if method != decodex_codex::schema::ACCOUNT_REFRESH_CALLBACK_METHOD {
			return self.write_json(&OutboundRpcError {
				id,
				error: OutboundRpcErrorBody {
					code: -32_601,
					message: "account-bound adapter does not service this request",
				},
			});
		}
		let request: ChatgptRefreshRequest =
			serde_json::from_slice(line).map_err(|_| SupervisionError::InvalidProtocol)?;
		if request.id != id
			|| request.method != decodex_codex::schema::ACCOUNT_REFRESH_CALLBACK_METHOD
			|| request.params.reason != "unauthorized"
			|| request.params.previous_account_id.as_ref().is_some_and(|value| {
				value.is_empty() || value.len() > 512 || value.chars().any(char::is_control)
			}) {
			return Err(SupervisionError::InvalidProtocol.into());
		}
		let account_id = self.binding.account_id.clone();
		let process_binding = self.binding.process_binding()?.clone();
		let callback = self
			.binding
			.refresh_callback
			.as_ref()
			.cloned()
			.ok_or(SupervisionError::LaunchCapabilityUnavailable)?;
		let projection = match callback.refresh(
			&account_id,
			&process_binding,
			&request.params.reason,
			request.params.previous_account_id.as_deref(),
		) {
			Ok(projection) => projection,
			Err(_) => {
				return self.write_json(&OutboundRpcError {
					id,
					error: OutboundRpcErrorBody {
						code: -32_001,
						message: "account credential refresh unavailable",
					},
				});
			},
		};
		if projection.provider_account_id != process_binding.credential.provider.account_id() {
			return Err(SupervisionError::AccountChanged.into());
		}
		self.write_json(&OutboundRpcSuccess {
			id,
			result: ChatgptRefreshResponse {
				access_token: projection.access_token.as_str(),
				chatgpt_account_id: projection.provider_account_id.as_str(),
				chatgpt_plan_type: projection.plan_type.as_deref(),
			},
		})
	}

	fn abandon_request(&mut self, request_id: u64) -> Result<(), RpcError> {
		if self.abandoned_request_ids.len() >= PROTOCOL_QUEUE_CAPACITY {
			return Err(RpcError::Supervision(SupervisionError::ProtocolLimitExceeded));
		}

		self.abandoned_request_ids.insert(request_id);

		Ok(())
	}

	fn notify<P>(&mut self, method: &str, params: &P) -> Result<(), ProbeError>
	where
		P: Serialize,
	{
		self.write_json(&OutboundNotification { method, params })
	}

	fn read_account_identity(&mut self, timeout: Duration) -> Result<AccountIdentity, ProbeError> {
		let account = self.request::<_, AccountReadResponse>(
			ReadOnlyMethod::AccountRead,
			&serde_json::json!({}),
			timeout,
		)?;
		let identity = account_identity(account)?;

		if self.expected_account_identity.as_ref().is_some_and(|expected| expected != &identity) {
			if self.shutdown_inner(ACCOUNT_MISMATCH_SHUTDOWN).is_err() {
				self.owner.transfer_to_reaper();

				return Err(SupervisionError::ShutdownFailed.into());
			}

			return Err(SupervisionError::AccountChanged.into());
		}

		self.expected_account_identity = Some(identity.clone());

		Ok(identity)
	}

	fn list_exact_threads(
		&mut self,
		filter: &ExactThreadListFilter,
		timeout: Duration,
	) -> Result<ExactThreadListResult, ExactReconciliationError> {
		self.re_attest_exact_account(timeout)?;

		let response = self
			.request_rpc::<_, ThreadListResponse>(
				"thread/list",
				&ExactThreadListParams {
					search_term: &filter.search_term,
					archived: filter.archived.as_bool(),
					limit: MAX_EXACT_THREAD_LIST_RESULTS as u32,
				},
				timeout,
			)
			.map_err(ExactReconciliationError::from_rpc)?;

		if response.data.len() > MAX_EXACT_THREAD_LIST_RESULTS {
			return Err(ExactReconciliationError::InvalidResult);
		}

		let threads = response
			.data
			.iter()
			.map(ExactThreadFacts::try_from)
			.collect::<Result<Vec<_>, _>>()
			.map_err(|_| ExactReconciliationError::InvalidResult)?;

		if threads.iter().any(|thread| {
			thread.archived != filter.archived.as_bool()
				|| thread.title.as_ref().map(|title| title.as_str())
					!= Some(filter.search_term.as_str())
		}) {
			return Err(ExactReconciliationError::InvalidResult);
		}

		self.re_attest_exact_account(timeout)?;

		ExactThreadListResult::from_protocol(threads)
			.map_err(|_| ExactReconciliationError::InvalidResult)
	}

	fn read_exact_thread(
		&mut self,
		thread_id: &ExactThreadId,
		timeout: Duration,
	) -> Result<ExactThreadReadResult, ExactReconciliationError> {
		self.re_attest_exact_account(timeout)?;

		let response = self
			.request_rpc::<_, ThreadReadResponse>(
				"thread/read",
				&ExactThreadReadParams { thread_id, include_turns: true },
				timeout,
			)
			.map_err(ExactReconciliationError::from_rpc)?;
		let facts = ExactThreadFacts::try_from(&response.thread)
			.map_err(|_| ExactReconciliationError::InvalidResult)?;

		if &facts.id != thread_id {
			return Err(ExactReconciliationError::InvalidResult);
		}

		self.re_attest_exact_account(timeout)?;

		Ok(ExactThreadReadResult { facts, history: LossyThreadHistory::IncludeTurnsReadback })
	}

	fn reconcile_archive(
		&mut self,
		thread_id: &ExactThreadId,
		timeout: Duration,
	) -> ArchiveReconciliationOutcome {
		let before = match self.read_exact_thread(thread_id, timeout) {
			Ok(readback) => readback,
			Err(error) => return error.archive_outcome(),
		};

		if before.facts.archived {
			return ArchiveReconciliationOutcome::AlreadyArchived;
		}

		let mutation = self.request_rpc::<_, ThreadArchiveResponse>(
			"thread/archive",
			&ThreadArchiveParams { thread_id },
			timeout,
		);

		if matches!(mutation, Err(RpcError::MethodRejected(-32_601))) {
			return ArchiveReconciliationOutcome::Unverified(
				ArchiveUnverifiedReason::MethodUnsupported,
			);
		}

		let mutation_confirmed = mutation.is_ok();

		match self.read_exact_thread(thread_id, timeout) {
			Ok(readback) if readback.facts.archived => ArchiveReconciliationOutcome::Archived,
			Ok(_) if mutation_confirmed =>
				ArchiveReconciliationOutcome::Unverified(ArchiveUnverifiedReason::ReadbackFailed),
			Ok(_) =>
				ArchiveReconciliationOutcome::Unverified(ArchiveUnverifiedReason::AmbiguousMutation),
			Err(error) => error.archive_outcome(),
		}
	}

	fn re_attest_exact_account(
		&mut self,
		timeout: Duration,
	) -> Result<(), ExactReconciliationError> {
		self.read_account_identity(timeout).map(|_| ()).map_err(|error| match error {
			ProbeError::Supervision(
				SupervisionError::AccountChanged | SupervisionError::ShutdownFailed,
			) => ExactReconciliationError::AccountBindingChanged,
			_ => ExactReconciliationError::Transport,
		})
	}

	fn write_json<T>(&mut self, value: &T) -> Result<(), ProbeError>
	where
		T: Serialize + ?Sized,
	{
		let frame = ZeroizingOutboundFrame::serialize(value)?;

		frame.write_to(&mut self.stdin)?;

		self.stdin.flush().map_err(|_| SupervisionError::WriteFailed.into())
	}

	fn shutdown_inner(&mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		self.owner.shutdown(timeout)
	}

	fn validate_zero_scratch_json(bytes: &[u8]) -> Result<(), ()> {
		let mut in_string = false;

		for &byte in bytes {
			match byte {
				b'"' => in_string = !in_string,
				b'\\' if in_string => return Err(()),
				0x00..=0x1f if in_string => return Err(()),
				_ => {},
			}
		}

		if in_string { Err(()) } else { Ok(()) }
	}
}

fn decode_quick_task_process_event(
	bytes: &[u8],
) -> Result<Option<QuickTaskProcessEvent>, QuickTaskProcessError> {
	if let Some(delta) =
		project_quick_task_message_delta(bytes).map_err(|_| QuickTaskProcessError::Incompatible)?
	{
		return Ok(Some(QuickTaskProcessEvent::MessageDelta(delta)));
	}
	match normalize_event(bytes).map_err(|_| QuickTaskProcessError::Incompatible)? {
		NormalizedEvent::TurnCompleted { turn_id, status, .. } =>
			Ok(Some(QuickTaskProcessEvent::TurnCompleted {
				turn_id: turn_id.as_str().to_owned(),
				status,
				witness_digest: hex_digest(&Sha256::digest(bytes)),
			})),
		_ => Ok(None),
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

fn spawn_protocol_process(
	command: &AppServerCommand,
	binding: &AccountBinding,
	guard: Option<RunnerPermit>,
	sender: SyncSender<InboundFrame>,
	protocol_limit_exceeded: Arc<AtomicBool>,
) -> Result<(ProcessGroupOwner, Box<dyn Write + Send>), SupervisionError> {
	#[cfg(target_os = "macos")]
	if let Some(identity) = &command.attested_code_identity {
		let home = binding.expected_codex_home.parent().ok_or(SupervisionError::InvalidBinding)?;
		let suspended =
			spawn_suspended(identity, &command.app_server_args, &command.working_directory, home)
				.map_err(|_| SupervisionError::SpawnFailed)?;

		// This non-profile path retains full filesystem and snapshot digest verification. The
		// dynamic code check then binds the stopped image to that snapshot before SIGCONT.
		verify_executable(command)?;

		let spawned =
			suspended.attest_and_resume(identity).map_err(|_| SupervisionError::SpawnFailed)?;
		let mut owner = ProcessGroupOwner::new(ManagedChild::Attested(spawned.child), guard);
		let pump = StdoutPump::start(spawned.stdout, sender, protocol_limit_exceeded)?;

		owner.attach_pump(pump);

		return Ok((owner, Box::new(spawned.stdin)));
	}

	let mut process = Command::new(protected_spawn_path(command));

	process
		.arg0(&command.program)
		.args(&command.app_server_args)
		.current_dir(&command.working_directory)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null());

	configure_child_environment(&mut process, binding)?;
	configure_process_session(&mut process, None);

	let child = process.spawn().map_err(|_| SupervisionError::SpawnFailed)?;
	let mut owner = ProcessGroupOwner::new(child, guard);
	let (stdin, stdout) = match owner.child_mut() {
		ManagedChild::Standard(child) => {
			let stdin = child.stdin.take().ok_or(SupervisionError::StdinUnavailable)?;
			let stdout = child.stdout.take().ok_or(SupervisionError::InvalidProtocol)?;

			(stdin, stdout)
		},
		#[cfg(target_os = "macos")]
		ManagedChild::Attested(_) => unreachable!("snapshot spawn created an attested child"),
	};
	let pump = StdoutPump::start(stdout, sender, protocol_limit_exceeded)?;

	owner.attach_pump(pump);

	Ok((owner, Box::new(stdin)))
}

fn spawn_attested_protocol_process(
	command: &AppServerCommand,
	binding: &AccountBinding,
	guard: RunnerPermit,
	capability: ExactBuildLaunchCapability,
	pre_spawn_check: Option<&dyn QuickTaskPreSpawnCheck>,
	sender: SyncSender<InboundFrame>,
	protocol_limit_exceeded: Arc<AtomicBool>,
) -> Result<(ProcessGroupOwner, Box<dyn Write + Send>), SupervisionError> {
	#[cfg(target_os = "macos")]
	{
		let identity = command
			.attested_code_identity
			.as_ref()
			.ok_or(SupervisionError::LaunchCapabilityUnavailable)?;
		let home = binding.expected_codex_home.parent().ok_or(SupervisionError::InvalidBinding)?;
		if let Some(check) = pre_spawn_check {
			check.validate_at_spawn_boundary().map_err(|()| SupervisionError::InvalidBinding)?;
		}
		let suspended = match (capability, pre_spawn_check) {
			(ExactBuildLaunchCapability::PrivateStdioDisabledEphemeralStartupV1, Some(check)) =>
				spawn_private_stdio_suspended_at(
					identity,
					&command.app_server_args,
					check.working_directory_descriptor(),
					home,
				),
			(ExactBuildLaunchCapability::PrivateStdioDisabledEphemeralStartupV1, None) =>
				spawn_private_stdio_suspended(
					identity,
					&command.app_server_args,
					&command.working_directory,
					home,
				),
		}
		.map_err(|_| SupervisionError::SpawnFailed)?;
		if let Some(check) = pre_spawn_check {
			check.validate_at_spawn_boundary().map_err(|()| SupervisionError::InvalidBinding)?;
		}

		// Startup already hashed and statically validated the immutable snapshot and canonical
		// image. Keep the canonical object identity stable here; exact dynamic CDHash and path
		// attestation below bind the suspended child to that startup profile before user code runs.
		verify_canonical_executable_identity(command)?;

		let spawned =
			suspended.attest_and_resume(identity).map_err(|_| SupervisionError::SpawnFailed)?;
		let mut owner = ProcessGroupOwner::new(ManagedChild::Attested(spawned.child), Some(guard));
		let pump = StdoutPump::start(spawned.stdout, sender, protocol_limit_exceeded)?;
		owner.attach_pump(pump);

		Ok((owner, Box::new(spawned.stdin)))
	}

	#[cfg(not(target_os = "macos"))]
	{
		let mut process = configured_attested_app_server_process(command, binding, capability)?;
		if let Some(check) = pre_spawn_check {
			check.validate_at_spawn_boundary().map_err(|()| SupervisionError::InvalidBinding)?;
		}
		let child = process.spawn().map_err(|_| SupervisionError::SpawnFailed)?;
		let mut owner = ProcessGroupOwner::new(child, Some(guard));
		let (stdin, stdout) = match owner.child_mut() {
			ManagedChild::Standard(child) => {
				let stdin = child.stdin.take().ok_or(SupervisionError::StdinUnavailable)?;
				let stdout = child.stdout.take().ok_or(SupervisionError::InvalidProtocol)?;

				(stdin, stdout)
			},
			#[cfg(target_os = "macos")]
			ManagedChild::Attested(_) => unreachable!("configured spawn created a standard child"),
		};
		let pump = StdoutPump::start(stdout, sender, protocol_limit_exceeded)?;
		owner.attach_pump(pump);

		Ok((owner, Box::new(stdin)))
	}
}

/// Typed result from `initialize` plus a bounded `thread/list`; no raw JSON escapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReadOnlyProbeResult {
	/// Exact-build negotiated capability profile.
	pub profile: CapabilityProfile,
	/// Redacted bounded thread summaries.
	pub threads: Vec<ThreadSummary>,
	/// Exact non-secret account selected before the child was spawned.
	pub account_id: AccountId,
	/// Exact OS process identity that produced the account readback.
	pub process_id: u32,
}

/// Fake/live probe that cannot construct a turn or account-selection request.
pub(super) struct ReadOnlyProbe {
	command: AppServerCommand,
	binding: AccountBinding,
	timeout: Duration,
	#[cfg(test)]
	attestation_timeout_override: Option<Duration>,
}
impl ReadOnlyProbe {
	/// Configure a probe. Schema validation is deferred to `run` but precedes spawn.
	pub fn new(command: AppServerCommand, binding: AccountBinding, timeout: Duration) -> Self {
		Self {
			command,
			binding,
			timeout,
			#[cfg(test)]
			attestation_timeout_override: None,
		}
	}

	/// Account named by this mechanical process binding.
	///
	/// This is caller-provided adapter configuration, not product readiness or launch authority.
	pub fn account_id(&self) -> &AccountId {
		self.binding.account_id()
	}

	#[cfg(test)]
	fn new_for_test(
		command: AppServerCommand,
		binding: AccountBinding,
		_schema_marker: SchemaMarker,
		timeout: Duration,
	) -> Self {
		Self { command, binding, timeout, attestation_timeout_override: None }
	}

	#[cfg(test)]
	fn with_attestation_timeout_for_test(mut self, timeout: Duration) -> Self {
		self.attestation_timeout_override = Some(timeout);

		self
	}

	/// Construct a synthetic probe using checked-in schema and process fixtures.
	#[cfg(test)]
	#[doc(hidden)]
	pub fn fixture(command: AppServerCommand, binding: AccountBinding, timeout: Duration) -> Self {
		Self { command, binding, timeout, attestation_timeout_override: None }
	}

	/// Test-only ambient probe retained for the landed XY-1270 capability fixtures.
	#[cfg(test)]
	fn run(self, cache: &mut CapabilityCache) -> Result<ReadOnlyProbeResult, ProbeError> {
		let guard = RunnerCapacity::try_with_limit(1)
			.map_err(|_| SupervisionError::CleanupUnavailable)?
			.reserve(self.binding.account_id.clone(), 1)
			.map_err(|_| SupervisionError::SpawnFailed)?;

		self.run_inner(None, Some(guard), cache)
	}

	/// Run the private mechanical path with the runtime owner's concrete capacity permit.
	pub(super) fn run_mechanical_with_lifetime_guard(
		self,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
		guard: RunnerPermit,
	) -> Result<ReadOnlyProbeResult, ProbeError> {
		self.run_inner(Some(vault), Some(guard), cache)
	}

	#[cfg(test)]
	fn run_bound_for_test(
		self,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
	) -> Result<ReadOnlyProbeResult, ProbeError> {
		let guard = RunnerCapacity::try_with_limit(1)
			.map_err(|_| SupervisionError::CleanupUnavailable)?
			.reserve(self.binding.account_id.clone(), 1)
			.map_err(|_| SupervisionError::SpawnFailed)?;

		self.run_inner(Some(vault), Some(guard), cache)
	}

	fn run_inner(
		self,
		vault: Option<&dyn CredentialVault>,
		guard: Option<RunnerPermit>,
		cache: &mut CapabilityCache,
	) -> Result<ReadOnlyProbeResult, ProbeError> {
		#[cfg(test)]
		let attestation_timeout = self.attestation_timeout_override.unwrap_or(self.timeout);
		#[cfg(not(test))]
		let attestation_timeout = self.timeout;
		let (build, generated, guard) =
			attest_executable(&self.command, &self.binding, attestation_timeout, guard)?;
		let mut negotiation = ProbeNegotiation::new(cache, &build, &generated);
		let account_id = self.binding.account_id.clone();
		let guard = guard.ok_or(SupervisionError::SpawnFailed)?;
		let spawned = SupervisedProcess::spawn_bound(self.command, self.binding, guard);
		let mut process = match spawned {
			Ok(process) => process,
			Err(error) => return negotiation.fail(Capability::Initialize, error.into()),
		};
		let _account_identity =
			initialize_probe(&mut process, vault, self.timeout, &mut negotiation)?;
		let list = probe_thread_list(&mut process, self.timeout, &mut negotiation)?;

		probe_thread_read(&mut process, &list, self.timeout, &mut negotiation)?;

		let profile = negotiation.cache_profile()?;
		let process_id = process.process_id();

		process.shutdown(Duration::from_secs(1))?;

		Ok(ReadOnlyProbeResult {
			profile,
			threads: list.data.iter().map(ThreadSummary::from).collect(),
			account_id,
			process_id,
		})
	}
}

/// One exact account-bound reset-card app-server run.
///
/// This mechanical owner does not select an account, resolve a public card descriptor, persist an
/// effect, or retry a consume request. Its consume entrypoint accepts only the exact provider
/// identifier and idempotency key that the runtime persisted before the external effect began.
#[derive(Clone)]
pub(super) struct ResetCardProcessRunner {
	command: AppServerCommand,
	binding: AccountBinding,
	timeout: Duration,
}
impl ResetCardProcessRunner {
	/// Configure one exact account-bound run. Schema attestation and process creation are deferred
	/// until an operation starts.
	pub(super) const fn new(
		command: AppServerCommand,
		binding: AccountBinding,
		timeout: Duration,
	) -> Self {
		Self { command, binding, timeout }
	}

	/// Exact non-secret account selected before process creation.
	pub(super) fn account_id(&self) -> &AccountId {
		self.binding.account_id()
	}

	/// Read one bounded strict inventory under an already-reserved runner permit.
	pub(super) fn read_inventory(
		self,
		vault: &dyn CredentialVault,
		guard: RunnerPermit,
	) -> Result<ResetCardInventory, ResetCardProcessError> {
		let timeout = self.timeout;
		let mut process = self.launch(vault, guard)?;
		let result = read_reset_card_inventory(&mut process, timeout).and_then(|inventory| {
			re_attest_reset_card_account(&mut process, timeout)?;

			Ok(inventory)
		});

		finish_reset_card_process(process, result)
	}

	/// Consume one already-persisted exact credit with its already-persisted key, then read a fresh
	/// bounded inventory before re-attesting the immutable account binding.
	pub(super) fn consume_and_readback(
		self,
		vault: &dyn CredentialVault,
		guard: RunnerPermit,
		credit_id: ExactResetCreditId,
		idempotency_key: ResetCardIdempotencyKey,
	) -> Result<ResetCardConsumeReadback, ResetCardProcessError> {
		let timeout = self.timeout;
		let mut process = self.launch(vault, guard)?;
		let result = consume_reset_card(&mut process, timeout, credit_id, idempotency_key)
			.and_then(|outcome| {
				let inventory = read_reset_card_inventory(&mut process, timeout)?;

				re_attest_reset_card_account(&mut process, timeout)?;

				Ok(ResetCardConsumeReadback { outcome, inventory })
			});

		finish_reset_card_process(process, result)
	}

	fn launch(
		self,
		vault: &dyn CredentialVault,
		guard: RunnerPermit,
	) -> Result<SupervisedProcess, ResetCardProcessError> {
		if &guard.account_id != self.binding.account_id() {
			return Err(ResetCardProcessError::AccountBindingChanged);
		}

		let (build, generated, guard) =
			attest_executable(&self.command, &self.binding, self.timeout, Some(guard))
				.map_err(ResetCardProcessError::from_probe)?;
		let reset_profile = ResetCardCapabilityProfile::from_schema(generated.contract());

		if !reset_profile.is_supported() {
			return Err(ResetCardProcessError::SchemaUnsupported(reset_profile.state()));
		}

		let guard = guard.ok_or(ResetCardProcessError::ProcessUnavailable)?;
		let mut process = SupervisedProcess::spawn_bound(self.command, self.binding, guard)
			.map_err(ResetCardProcessError::from_supervision)?;
		let mut cache = CapabilityCache::default();
		let mut negotiation = ProbeNegotiation::new(&mut cache, &build, &generated);

		if let Err(error) =
			initialize_probe(&mut process, Some(vault), self.timeout, &mut negotiation)
		{
			let mapped = ResetCardProcessError::from_probe(error);

			return finish_reset_card_process(process, Err(mapped));
		}

		Ok(process)
	}
}

impl Debug for ResetCardProcessRunner {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("ResetCardProcessRunner").finish_non_exhaustive()
	}
}

/// Terminal provider outcome plus the immediate strict inventory readback.
pub(super) struct ResetCardConsumeReadback {
	pub(super) outcome: ResetCardConsumeOutcome,
	pub(super) inventory: ResetCardInventory,
}
impl Debug for ResetCardConsumeReadback {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ResetCardConsumeReadback")
			.field("outcome", &self.outcome)
			.field("inventory", &self.inventory)
			.finish()
	}
}

/// Closed reset-card app-server method classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResetCardProcessMethod {
	InventoryRead,
	Consume,
}

/// Sanitized reset-card process failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResetCardProcessError {
	/// The generated schema was malformed or lacked the shared app-server baseline.
	SchemaInvalid,
	/// The exact generated schema did not advertise both reset-card methods.
	SchemaUnsupported(ResetCardCapabilityState),
	/// The selected account credential could not be projected into the child.
	CredentialVault(CredentialVaultError),
	/// Account identity changed before the operation readback completed.
	AccountBindingChanged,
	/// The exact generated method was advertised but rejected at runtime.
	MethodUnavailable(ResetCardProcessMethod),
	/// An app-server response did not satisfy the strict typed provider contract.
	InvalidProviderResponse,
	/// Executable, process, or bounded transport mechanics were unavailable.
	ProcessUnavailable,
	/// Bounded process-group shutdown could not be confirmed.
	ShutdownFailed,
}
impl ResetCardProcessError {
	fn from_probe(error: ProbeError) -> Self {
		match error {
			ProbeError::SchemaMissing { .. } => Self::SchemaInvalid,
			ProbeError::CredentialVault(error) => Self::CredentialVault(error),
			ProbeError::Supervision(SupervisionError::AccountChanged) =>
				Self::AccountBindingChanged,
			ProbeError::Supervision(SupervisionError::InvalidProtocol)
			| ProbeError::Supervision(SupervisionError::ProtocolLimitExceeded) =>
				Self::InvalidProviderResponse,
			ProbeError::Supervision(SupervisionError::ShutdownFailed) => Self::ShutdownFailed,
			ProbeError::MethodRejected { .. } => Self::ProcessUnavailable,
			ProbeError::CapabilityConflict | ProbeError::Supervision(_) => Self::ProcessUnavailable,
		}
	}

	fn from_supervision(error: SupervisionError) -> Self {
		match error {
			SupervisionError::AccountChanged => Self::AccountBindingChanged,
			SupervisionError::InvalidProtocol | SupervisionError::ProtocolLimitExceeded =>
				Self::InvalidProviderResponse,
			SupervisionError::ShutdownFailed => Self::ShutdownFailed,
			_ => Self::ProcessUnavailable,
		}
	}

	fn from_rpc(error: RpcError, method: ResetCardProcessMethod) -> Self {
		match error {
			RpcError::MethodRejected(-32_601) => Self::MethodUnavailable(method),
			RpcError::MethodRejected(_) => Self::ProcessUnavailable,
			RpcError::Supervision(error) => Self::from_supervision(error),
		}
	}
}

fn read_reset_card_inventory(
	process: &mut SupervisedProcess,
	timeout: Duration,
) -> Result<ResetCardInventory, ResetCardProcessError> {
	let inventory = request_reset_card_inventory(process, timeout)?;
	if !inventory.details_complete() {
		return request_reset_card_inventory(process, timeout);
	}
	Ok(inventory)
}

fn request_reset_card_inventory(
	process: &mut SupervisedProcess,
	timeout: Duration,
) -> Result<ResetCardInventory, ResetCardProcessError> {
	process.request_rpc(RESET_CARD_READ_METHOD, &(), timeout).map_err(|error| {
		ResetCardProcessError::from_rpc(error, ResetCardProcessMethod::InventoryRead)
	})
}

fn consume_reset_card(
	process: &mut SupervisedProcess,
	timeout: Duration,
	credit_id: ExactResetCreditId,
	idempotency_key: ResetCardIdempotencyKey,
) -> Result<ResetCardConsumeOutcome, ResetCardProcessError> {
	process
		.request_rpc::<_, ResetCardConsumeResult>(
			RESET_CARD_CONSUME_METHOD,
			&ResetCardConsumeParams::new(credit_id, idempotency_key),
			timeout,
		)
		.map(ResetCardConsumeResult::outcome)
		.map_err(|error| ResetCardProcessError::from_rpc(error, ResetCardProcessMethod::Consume))
}

fn re_attest_reset_card_account(
	process: &mut SupervisedProcess,
	timeout: Duration,
) -> Result<(), ResetCardProcessError> {
	process.read_account_identity(timeout).map(|_| ()).map_err(ResetCardProcessError::from_probe)
}

fn finish_reset_card_process<T>(
	process: SupervisedProcess,
	result: Result<T, ResetCardProcessError>,
) -> Result<T, ResetCardProcessError> {
	process
		.shutdown(RESET_CARD_SHUTDOWN_WAIT)
		.map_err(|_| ResetCardProcessError::ShutdownFailed)?;

	result
}

/// Private account-bound exact reconciliation configuration.
///
/// This is deliberately separate from [`ReadOnlyProbe`]: archive is never part of capability-probe
/// execution, and no public caller can construct or dispatch this owner.
pub(super) struct ExactThreadReconciler {
	command: AppServerCommand,
	binding: AccountBinding,
	timeout: Duration,
}
impl ExactThreadReconciler {
	pub(super) fn new(
		command: AppServerCommand,
		binding: AccountBinding,
		timeout: Duration,
	) -> Self {
		Self { command, binding, timeout }
	}

	pub(super) fn account_id(&self) -> &AccountId {
		self.binding.account_id()
	}

	#[cfg(test)]
	fn fixture(command: AppServerCommand, binding: AccountBinding, timeout: Duration) -> Self {
		Self { command, binding, timeout }
	}

	pub(super) fn run_mechanical_with_lifetime_guard(
		self,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
		guard: RunnerPermit,
		operation: ExactThreadReconciliation,
	) -> Result<ExactThreadReconciliationResult, ExactThreadReconciliationFailure> {
		let (build, generated, guard) =
			attest_executable(&self.command, &self.binding, self.timeout, Some(guard))
				.map_err(ExactThreadReconciliationFailure::Probe)?;
		let required_method = match &operation {
			ExactThreadReconciliation::List(_) => "thread/list",
			ExactThreadReconciliation::Read(_) => "thread/read",
			ExactThreadReconciliation::Archive(_) => "thread/archive",
		};

		if !generated.contract().advertises_request(required_method) {
			return match operation {
				ExactThreadReconciliation::Archive(_) =>
					Ok(ExactThreadReconciliationResult::Archive(
						ArchiveReconciliationOutcome::Unverified(
							ArchiveUnverifiedReason::MethodUnsupported,
						),
					)),
				_ => Err(ExactThreadReconciliationFailure::Operation(
					ExactReconciliationError::MethodUnsupported,
				)),
			};
		}

		let guard = guard
			.ok_or(ExactThreadReconciliationFailure::Probe(SupervisionError::SpawnFailed.into()))?;
		let mut negotiation = ProbeNegotiation::new(cache, &build, &generated);
		let mut process = SupervisedProcess::spawn_bound(self.command, self.binding, guard)
			.map_err(|error| ExactThreadReconciliationFailure::Probe(error.into()))?;

		initialize_probe(&mut process, Some(vault), self.timeout, &mut negotiation)
			.map_err(ExactThreadReconciliationFailure::Probe)?;

		let result = match operation {
			ExactThreadReconciliation::List(filter) => ExactThreadReconciliationResult::List(
				process
					.list_exact_threads(&filter, self.timeout)
					.map_err(ExactThreadReconciliationFailure::Operation)?,
			),
			ExactThreadReconciliation::Read(thread_id) => ExactThreadReconciliationResult::Read(
				process
					.read_exact_thread(&thread_id, self.timeout)
					.map_err(ExactThreadReconciliationFailure::Operation)?,
			),
			ExactThreadReconciliation::Archive(thread_id) =>
				ExactThreadReconciliationResult::Archive(
					process.reconcile_archive(&thread_id, self.timeout),
				),
		};

		process
			.shutdown(Duration::from_secs(1))
			.map_err(ExactThreadReconciliationFailure::Shutdown)?;

		Ok(result)
	}
}

pub(super) enum ExactThreadReconciliation {
	List(ExactThreadListFilter),
	Read(ExactThreadId),
	Archive(ExactThreadId),
}

pub(super) enum ExactThreadReconciliationResult {
	List(ExactThreadListResult),
	Read(ExactThreadReadResult),
	Archive(ArchiveReconciliationOutcome),
}

#[derive(Debug)]
pub(super) enum ExactThreadReconciliationFailure {
	Probe(ProbeError),
	Operation(ExactReconciliationError),
	Shutdown(SupervisionError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExactReconciliationError {
	Transport,
	MethodUnsupported,
	InvalidResult,
	AccountBindingChanged,
}
impl ExactReconciliationError {
	fn from_rpc(error: RpcError) -> Self {
		match error {
			RpcError::MethodRejected(-32_601) => Self::MethodUnsupported,
			RpcError::MethodRejected(_) | RpcError::Supervision(_) => Self::Transport,
		}
	}

	const fn archive_outcome(self) -> ArchiveReconciliationOutcome {
		let reason = match self {
			Self::MethodUnsupported => ArchiveUnverifiedReason::MethodUnsupported,
			Self::AccountBindingChanged => ArchiveUnverifiedReason::AccountBindingChanged,
			Self::Transport | Self::InvalidResult => ArchiveUnverifiedReason::ReadbackFailed,
		};

		ArchiveReconciliationOutcome::Unverified(reason)
	}
}

/// Closed credential-vault failure without secret, provider, or account text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialVaultError {
	/// The selected account has no usable host-vault entry.
	Unavailable,
	/// The child rejected the process-scoped credential projection.
	ProjectionRejected,
	/// A vault attempted to switch credentials under one live child.
	ProjectionAlreadyUsed,
}
impl std::error::Error for CredentialVaultError {}

impl Display for CredentialVaultError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
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
	/// The exact image cannot attest the required private-stdio launch capability.
	LaunchCapabilityUnavailable,
	/// Child could not be spawned.
	SpawnFailed,
	/// Autonomous bounded cleanup ownership could not be established.
	CleanupUnavailable,
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
	/// A different profile already occupied this observed-executable cache key.
	CapabilityConflict,
	/// The selected host-vault credential could not be projected into this child.
	CredentialVault(CredentialVaultError),
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
	/// Project one selected account before immutable readback attestation.
	AccountLoginStart,
	/// Read the immutable process account.
	AccountRead,
	/// Read a bounded page of threads.
	ThreadList,
	/// Read one exact thread without turns.
	ThreadRead,
}
impl ReadOnlyMethod {
	fn as_str(self) -> &'static str {
		match self {
			Self::Initialize => "initialize",
			Self::AccountLoginStart => "account/login/start",
			Self::AccountRead => "account/read",
			Self::ThreadList => "thread/list",
			Self::ThreadRead => "thread/read",
		}
	}
}

#[derive(Serialize)]
pub(super) struct OutboundRequest<'a, P>
where
	P: ?Sized,
{
	id: u64,
	method: &'static str,
	params: &'a P,
}

fn exact_request_frame<P>(
	request_id: u64,
	method: &'static str,
	params: &P,
) -> Result<ZeroizingOutboundFrame, ProbeError>
where
	P: Serialize,
{
	ZeroizingOutboundFrame::serialize(&OutboundRequest { id: request_id, method, params })
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Serialize)]
pub(super) struct OutboundNotification<'a, P>
where
	P: ?Sized,
{
	method: &'a str,
	params: &'a P,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChatgptAuthParams<'a> {
	#[serde(rename = "type")]
	kind: &'static str,
	access_token: &'a str,
	chatgpt_account_id: &'a str,
	chatgpt_plan_type: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CredentialProjectionResponse {
	#[serde(rename = "type")]
	_kind: CredentialProjectionResponseKind,
}

#[derive(Deserialize)]
enum CredentialProjectionResponseKind {
	#[serde(rename = "chatgptAuthTokens")]
	ChatgptAuthTokens,
}

#[derive(Deserialize)]
pub(super) struct InboundHeader {
	id: Option<u64>,
	method: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatgptRefreshRequest {
	id: u64,
	method: String,
	params: ChatgptRefreshParams,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ChatgptRefreshParams {
	reason: String,
	previous_account_id: Option<String>,
}

#[derive(Serialize)]
struct OutboundRpcSuccess<T> {
	id: u64,
	result: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatgptRefreshResponse<'a> {
	access_token: &'a str,
	chatgpt_account_id: &'a str,
	chatgpt_plan_type: Option<&'a str>,
}

#[derive(Serialize)]
struct OutboundRpcError {
	id: u64,
	error: OutboundRpcErrorBody,
}

#[derive(Serialize)]
struct OutboundRpcErrorBody {
	code: i32,
	message: &'static str,
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct PreflightCleanupTest {
	trigger_spawn: u32,
	spawn_count: Arc<AtomicU32>,
	process_group: Arc<AtomicU32>,
	reaper_delay: Duration,
	quarantine: Arc<ProcessQuarantine>,
}

pub(super) struct ProbeNegotiation<'a> {
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

enum ManagedChild {
	Standard(Child),
	#[cfg(target_os = "macos")]
	Attested(AttestedChild),
}
impl ManagedChild {
	fn id(&self) -> u32 {
		match self {
			Self::Standard(child) => child.id(),
			#[cfg(target_os = "macos")]
			Self::Attested(child) => child.id(),
		}
	}

	fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
		match self {
			Self::Standard(child) => child.try_wait(),
			#[cfg(target_os = "macos")]
			Self::Attested(child) => child.try_wait(),
		}
	}
}
impl From<Child> for ManagedChild {
	fn from(child: Child) -> Self {
		Self::Standard(child)
	}
}

pub(super) struct ReapJob {
	child: ManagedChild,
	process_group: u32,
	// PID/PGID reuse makes signaling invalid after positive reap or an uncertain wait error.
	may_signal_process_group: bool,
	pump: Option<StdoutPump>,
	_guard: Option<RunnerPermit>,
	#[cfg(test)]
	not_before: Option<Instant>,
}

pub(super) struct ProcessGroupOwner {
	child: Option<ManagedChild>,
	process_group: u32,
	// This authority is monotonic: no observation can restore it after reap or wait failure.
	may_signal_process_group: bool,
	guard: Option<RunnerPermit>,
	pump: Option<StdoutPump>,
	#[cfg(test)]
	reap_not_before: Option<Instant>,
}
impl ProcessGroupOwner {
	fn new(child: impl Into<ManagedChild>, guard: Option<RunnerPermit>) -> Self {
		let child = child.into();
		let process_group = child.id();

		Self {
			child: Some(child),
			process_group,
			may_signal_process_group: true,
			guard,
			pump: None,
			#[cfg(test)]
			reap_not_before: None,
		}
	}

	fn attach_pump(&mut self, pump: StdoutPump) {
		debug_assert!(self.pump.is_none());

		self.pump = Some(pump);
	}

	fn child_mut(&mut self) -> &mut ManagedChild {
		self.child.as_mut().expect("live owner must retain its child")
	}

	fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
		let result = self.child_mut().try_wait();

		if !matches!(&result, Ok(None)) {
			self.may_signal_process_group = false;
		}

		result
	}

	const fn may_signal_process_group(&self) -> bool {
		self.may_signal_process_group
	}

	fn process_id(&self) -> u32 {
		self.process_group
	}

	fn shutdown(&mut self, timeout: Duration) -> Result<ShutdownOutcome, SupervisionError> {
		let (outcome, guard) = self.shutdown_retaining_guard(timeout)?;

		drop(guard);

		Ok(outcome)
	}

	fn shutdown_retaining_guard(
		&mut self,
		timeout: Duration,
	) -> Result<(ShutdownOutcome, Option<RunnerPermit>), SupervisionError> {
		let Some(child) = self.child.as_mut() else {
			return Ok((ShutdownOutcome::Exited, self.guard.take()));
		};
		let outcome = terminate_process_group(
			child,
			self.process_group,
			timeout,
			&mut self.may_signal_process_group,
		)?;

		if self
			.pump
			.as_mut()
			.is_some_and(|pump| !pump.stop(timeout.min(Duration::from_millis(250))))
		{
			return Err(SupervisionError::ShutdownFailed);
		}

		self.pump = None;

		let mut child = self.child.take().expect("confirmed child was present");

		let result = child.try_wait();

		if !matches!(result, Ok(Some(_))) {
			self.may_signal_process_group = false;
			self.child = Some(child);

			return Err(SupervisionError::ShutdownFailed);
		}

		Ok((outcome, self.guard.take()))
	}

	fn transfer_to_reaper(&mut self) {
		let Some(child) = self.child.take() else {
			return;
		};
		let mut guard =
			self.guard.take().expect("every spawned group owns capacity and a cleanup slot");
		let (quarantine, slot) = guard.quarantine();
		let job = ReapJob {
			child,
			process_group: self.process_group,
			may_signal_process_group: self.may_signal_process_group,
			pump: self.pump.take(),
			_guard: Some(guard),
			#[cfg(test)]
			not_before: self.reap_not_before,
		};

		quarantine.submit(slot, job);
	}
}

impl Drop for ProcessGroupOwner {
	fn drop(&mut self) {
		if self.child.is_some() && self.shutdown(Duration::from_millis(250)).is_err() {
			self.transfer_to_reaper();
		}
	}
}

/// One fixed allocation that explicitly wipes its complete contents before release.
pub(super) struct ZeroizingInboundBlock {
	bytes: Box<[u8]>,
}
impl ZeroizingInboundBlock {
	fn new() -> Self {
		Self { bytes: vec![0; INBOUND_BLOCK_BYTES].into_boxed_slice() }
	}

	fn wipe(&mut self) {
		self.bytes.zeroize();
	}
}
impl Drop for ZeroizingInboundBlock {
	fn drop(&mut self) {
		self.wipe();
		#[cfg(test)]
		ZEROIZED_INBOUND_BLOCKS.fetch_add(1, Ordering::AcqRel);
	}
}

/// Chunked inbound frame. Secret-bearing allocations never grow or reallocate.
pub(super) struct InboundFrame {
	blocks: Vec<ZeroizingInboundBlock>,
	len: usize,
}
impl InboundFrame {
	fn new() -> Self {
		Self { blocks: Vec::new(), len: 0 }
	}

	fn is_empty(&self) -> bool {
		self.len == 0
	}

	fn extend_from_slice(&mut self, mut bytes: &[u8]) -> Result<(), ()> {
		if self.len.saturating_add(bytes.len()) > MAX_APP_SERVER_FRAME_BYTES {
			return Err(());
		}

		while !bytes.is_empty() {
			let offset = self.len % INBOUND_BLOCK_BYTES;

			if offset == 0 {
				self.blocks.push(ZeroizingInboundBlock::new());
			}

			let count = bytes.len().min(INBOUND_BLOCK_BYTES - offset);
			let block = self.blocks.last_mut().expect("a frame block was just installed");

			block.bytes[offset..offset + count].copy_from_slice(&bytes[..count]);

			self.len += count;
			bytes = &bytes[count..];
		}

		Ok(())
	}

	fn into_contiguous(self) -> Zeroizing<Vec<u8>> {
		let mut bytes = Vec::with_capacity(self.len);
		let mut remaining = self.len;

		for block in &self.blocks {
			let count = remaining.min(INBOUND_BLOCK_BYTES);

			bytes.extend_from_slice(&block.bytes[..count]);

			remaining -= count;
		}

		debug_assert_eq!(remaining, 0);

		Zeroizing::new(bytes)
	}
}

pub(super) struct QuarantineSlotLease {
	state: Arc<ProcessQuarantineState>,
	index: usize,
	installed: bool,
}
impl QuarantineSlotLease {
	pub(super) fn index(&self) -> usize {
		self.index
	}

	pub(super) fn mark_installed(&mut self) {
		self.installed = true;
	}
}
impl Debug for QuarantineSlotLease {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("QuarantineSlotLease").finish_non_exhaustive()
	}
}
impl Drop for QuarantineSlotLease {
	fn drop(&mut self) {
		if !self.installed {
			let previous =
				self.state.slots[self.index].state.swap(QUARANTINE_SLOT_FREE, Ordering::AcqRel);

			debug_assert_eq!(previous, QUARANTINE_SLOT_RESERVED);
		}
	}
}

pub(super) struct QuarantineSlot {
	state: AtomicU8,
	job: UnsafeCell<MaybeUninit<ReapJob>>,
}
// SAFETY: `state` grants exclusive access to `job`: only the reservation owner writes RESERVED,
// one worker may transition READY to WORKING and read it, and that worker either restores READY
// or clears the slot after dropping the job. No state permits concurrent `job` access.
unsafe impl Sync for QuarantineSlot {}
impl QuarantineSlot {
	const fn new() -> Self {
		Self {
			state: AtomicU8::new(QUARANTINE_SLOT_FREE),
			job: UnsafeCell::new(MaybeUninit::uninit()),
		}
	}
}

pub(super) struct ProcessQuarantine {
	state: Arc<ProcessQuarantineState>,
	shutdown: SyncSender<()>,
	joined: Mutex<Receiver<()>>,
	worker_id: ThreadId,
}
impl ProcessQuarantine {
	pub(super) fn new() -> Arc<Self> {
		Self::try_new().expect("the cleanup owner must exist before test capacity")
	}

	pub(super) fn try_new() -> Result<Arc<Self>, SupervisionError> {
		Self::try_new_inner(QuarantineStartFailure::None)
	}

	#[cfg(test)]
	fn try_new_with_worker_start_failure() -> Result<Arc<Self>, SupervisionError> {
		Self::try_new_inner(QuarantineStartFailure::Worker)
	}

	#[cfg(test)]
	fn try_new_with_coordinator_start_failure(
		lifecycle: Arc<QuarantineLifecycleProbe>,
	) -> Result<Arc<Self>, SupervisionError> {
		Self::try_new_inner_with_lifecycle(QuarantineStartFailure::Coordinator, lifecycle)
	}

	fn try_new_inner(failure: QuarantineStartFailure) -> Result<Arc<Self>, SupervisionError> {
		let lifecycle = Arc::new(QuarantineLifecycleProbe::default());

		Self::try_new_inner_with_lifecycle(failure, lifecycle)
	}

	fn try_new_inner_with_lifecycle(
		failure: QuarantineStartFailure,
		lifecycle: Arc<QuarantineLifecycleProbe>,
	) -> Result<Arc<Self>, SupervisionError> {
		let state = Arc::new(ProcessQuarantineState {
			slots: (0..MAX_PROCESS_QUARANTINE)
				.map(|_| QuarantineSlot::new())
				.collect::<Vec<_>>()
				.into_boxed_slice(),
			ready: Condvar::new(),
			wake: Mutex::new(()),
			next_slot: AtomicUsize::new(0),
			worker_cursor: AtomicUsize::new(0),
			shutdown: AtomicBool::new(false),
			#[cfg(test)]
			panic_after_worker_pops: AtomicUsize::new(0),
			lifecycle,
		});

		if matches!(failure, QuarantineStartFailure::Worker) {
			return Err(SupervisionError::CleanupUnavailable);
		}

		let worker_state = Arc::clone(&state);
		let (worker_started, worker_started_receiver) = mpsc::sync_channel(1);
		let worker = Builder::new()
			.name("decodex-runtime-process-quarantine".into())
			.spawn(move || {
				worker_state.lifecycle.started.store(true, Ordering::Release);

				let _ = worker_started.send(());

				worker_state.worker_loop();
			})
			.map_err(|_| SupervisionError::CleanupUnavailable)?;
		let worker_id = worker.thread().id();
		let (shutdown, shutdown_request) = mpsc::sync_channel(1);
		let (joined_notice, joined) = mpsc::sync_channel(1);
		let (worker_handle, worker_handle_receiver) = mpsc::sync_channel::<JoinHandle<()>>(1);
		let coordinator_state = Arc::clone(&state);

		if matches!(failure, QuarantineStartFailure::Coordinator) {
			state.shutdown.store(true, Ordering::Release);
			state.ready.notify_all();

			let _ = worker.join();

			state.lifecycle.joined.store(true, Ordering::Release);

			return Err(SupervisionError::CleanupUnavailable);
		}

		let coordinator = Builder::new()
			.name("decodex-runtime-process-quarantine-join".into())
			.spawn(move || {
				let Ok(worker) = worker_handle_receiver.recv() else {
					return;
				};
				let _ = shutdown_request.recv();

				coordinator_state.shutdown.store(true, Ordering::Release);
				coordinator_state.ready.notify_all();

				let _ = worker.join();

				coordinator_state.lifecycle.joined.store(true, Ordering::Release);

				let _ = joined_notice.send(());
			});
		let coordinator = match coordinator {
			Ok(coordinator) => coordinator,
			Err(_) => {
				state.shutdown.store(true, Ordering::Release);
				state.ready.notify_all();

				let _ = worker.join();

				state.lifecycle.joined.store(true, Ordering::Release);

				return Err(SupervisionError::CleanupUnavailable);
			},
		};

		if let Err(error) = worker_handle.send(worker) {
			state.shutdown.store(true, Ordering::Release);
			state.ready.notify_all();

			let _ = error.0.join();

			state.lifecycle.joined.store(true, Ordering::Release);

			let _ = coordinator.join();

			return Err(SupervisionError::CleanupUnavailable);
		}

		if worker_started_receiver.recv_timeout(QUARANTINE_SHUTDOWN_WAIT).is_err() {
			let _ = shutdown.try_send(());
			let _ = joined.recv_timeout(QUARANTINE_SHUTDOWN_WAIT);

			return Err(SupervisionError::CleanupUnavailable);
		}

		drop(coordinator);

		Ok(Arc::new(Self { state, shutdown, joined: Mutex::new(joined), worker_id }))
	}

	pub(super) fn reserve_slot(self: &Arc<Self>) -> Option<QuarantineSlotLease> {
		let start = self.state.next_slot.fetch_add(1, Ordering::Relaxed) % self.state.slots.len();

		(0..self.state.slots.len()).find_map(|offset| {
			let index = (start + offset) % self.state.slots.len();

			self.state.slots[index]
				.state
				.compare_exchange(
					QUARANTINE_SLOT_FREE,
					QUARANTINE_SLOT_RESERVED,
					Ordering::AcqRel,
					Ordering::Acquire,
				)
				.ok()
				.map(|_| QuarantineSlotLease {
					state: Arc::clone(&self.state),
					index,
					installed: false,
				})
		})
	}

	fn submit(self: &Arc<Self>, index: usize, job: ReapJob) {
		let slot = &self.state.slots[index];

		debug_assert_eq!(slot.state.load(Ordering::Acquire), QUARANTINE_SLOT_RESERVED);

		// SAFETY: the reservation owns RESERVED exclusively and workers only read READY.
		unsafe { (*slot.job.get()).write(job) };
		slot.state.store(QUARANTINE_SLOT_READY, Ordering::Release);
		self.state.ready.notify_one();
	}

	#[cfg(test)]
	fn lifecycle_probe(&self) -> Arc<QuarantineLifecycleProbe> {
		Arc::clone(&self.state.lifecycle)
	}

	#[cfg(test)]
	fn panic_after_next_worker_pop(&self) {
		self.state.panic_after_worker_pops.store(1, Ordering::Release);
	}
}

impl Drop for ProcessQuarantine {
	fn drop(&mut self) {
		let _ = self.shutdown.try_send(());

		if thread::current().id() != self.worker_id {
			let joined = self.joined.lock().unwrap_or_else(PoisonError::into_inner);
			let _ = joined.recv_timeout(QUARANTINE_SHUTDOWN_WAIT);
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RpcError {
	Supervision(SupervisionError),
	MethodRejected(i64),
}

struct RpcWireReceipt {
	request_id: i64,
	request_digest: String,
	response_id: i64,
	response_digest: String,
}

struct RpcSuccess<T> {
	value: T,
	wire: RpcWireReceipt,
}

struct ProcessQuarantineState {
	slots: Box<[QuarantineSlot]>,
	ready: Condvar,
	wake: Mutex<()>,
	next_slot: AtomicUsize,
	worker_cursor: AtomicUsize,
	shutdown: AtomicBool,
	#[cfg(test)]
	panic_after_worker_pops: AtomicUsize,
	lifecycle: Arc<QuarantineLifecycleProbe>,
}
impl ProcessQuarantineState {
	fn worker_loop(self: Arc<Self>) {
		while !self.shutdown.load(Ordering::Acquire) {
			let _ = panic::catch_unwind(AssertUnwindSafe(|| self.worker_iteration()));
		}

		self.lifecycle.exited.store(true, Ordering::Release);
	}

	fn worker_iteration(self: &Arc<Self>) {
		let Some((index, job)) = self.take_ready() else {
			let wake = self.wake.lock().unwrap_or_else(PoisonError::into_inner);
			let _ = self.ready.wait_timeout(wake, Duration::from_millis(25));

			return;
		};
		let mut in_flight = InFlightReapJob::new(Arc::clone(self), index, job);

		#[cfg(test)]
		if self
			.panic_after_worker_pops
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| remaining.checked_sub(1))
			.is_ok()
		{
			panic!("injected quarantine worker panic after pop");
		}

		let completed = panic::catch_unwind(AssertUnwindSafe(|| {
			cleanup_process_group_once(in_flight.job_mut())
		}))
		.unwrap_or(false);

		if completed {
			in_flight.complete();
		} else {
			in_flight.requeue();
		}

		thread::sleep(Duration::from_millis(25));
	}

	fn take_ready(&self) -> Option<(usize, ReapJob)> {
		let start = self.worker_cursor.load(Ordering::Relaxed) % self.slots.len();

		(0..self.slots.len()).find_map(|offset| {
			let index = (start + offset) % self.slots.len();
			let slot = &self.slots[index];

			slot.state
				.compare_exchange(
					QUARANTINE_SLOT_READY,
					QUARANTINE_SLOT_WORKING,
					Ordering::AcqRel,
					Ordering::Acquire,
				)
				.ok()
				.map(|_| {
					self.worker_cursor.store((index + 1) % self.slots.len(), Ordering::Relaxed);

					// SAFETY: WORKING is exclusively owned by this worker after the CAS.
					let job = unsafe { (*slot.job.get()).assume_init_read() };

					(index, job)
				})
		})
	}
}

struct InFlightReapJob {
	state: Arc<ProcessQuarantineState>,
	index: usize,
	job: Option<ReapJob>,
}
impl InFlightReapJob {
	fn new(state: Arc<ProcessQuarantineState>, index: usize, job: ReapJob) -> Self {
		Self { state, index, job: Some(job) }
	}

	fn job_mut(&mut self) -> &mut ReapJob {
		self.job.as_mut().expect("in-flight cleanup retains one job")
	}

	fn complete(mut self) {
		drop(self.job.take());

		self.state.slots[self.index].state.store(QUARANTINE_SLOT_FREE, Ordering::Release);
	}

	fn requeue(mut self) {
		let job = self.job.take().expect("in-flight cleanup retains one job");
		let slot = &self.state.slots[self.index];

		// SAFETY: this worker exclusively owns WORKING for this slot.
		unsafe { (*slot.job.get()).write(job) };
		slot.state.store(QUARANTINE_SLOT_READY, Ordering::Release);
		self.state.ready.notify_one();
	}
}
impl Drop for InFlightReapJob {
	fn drop(&mut self) {
		if let Some(job) = self.job.take() {
			let slot = &self.state.slots[self.index];

			// SAFETY: unwind retains exclusive WORKING ownership.
			unsafe { (*slot.job.get()).write(job) };
			slot.state.store(QUARANTINE_SLOT_READY, Ordering::Release);
			self.state.ready.notify_one();
		}
	}
}

#[derive(Default)]
struct QuarantineLifecycleProbe {
	started: AtomicBool,
	exited: AtomicBool,
	joined: AtomicBool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum QuarantineStartFailure {
	None,
	Worker,
	Coordinator,
}

struct ExecutableSnapshot {
	#[cfg(target_os = "macos")]
	_directory: TempDir,
	#[cfg(target_os = "macos")]
	path: PathBuf,
	#[cfg(target_os = "linux")]
	file: File,
	source_device: u64,
	source_inode: u64,
}
impl ExecutableSnapshot {
	fn execution_path(&self) -> PathBuf {
		#[cfg(target_os = "macos")]
		return self.path.clone();

		#[cfg(target_os = "linux")]
		return linux_execution_path(&self.file);
	}

	fn digest(&self) -> Result<[u8; 32], SupervisionError> {
		#[cfg(target_os = "macos")]
		return executable_digest(&self.path);

		#[cfg(target_os = "linux")]
		return executable_digest_file(&self.file);
	}
}
#[cfg(target_os = "macos")]
impl Drop for ExecutableSnapshot {
	fn drop(&mut self) {
		let _ = set_snapshot_immutable(&self.path, false);
	}
}

#[cfg(test)]
#[derive(Clone)]
struct BeforeSpawnTest {
	trigger_spawn: u32,
	spawn_count: Arc<AtomicU32>,
	action: Arc<dyn Fn() + Send + Sync>,
}

struct ZeroizingOutboundBlock {
	bytes: Box<[u8]>,
}
impl ZeroizingOutboundBlock {
	fn new() -> Self {
		Self { bytes: vec![0; OUTBOUND_BLOCK_BYTES].into_boxed_slice() }
	}
}
impl Drop for ZeroizingOutboundBlock {
	fn drop(&mut self) {
		self.bytes.zeroize();
		#[cfg(test)]
		ZEROIZED_OUTBOUND_BLOCKS.fetch_add(1, Ordering::AcqRel);
	}
}

struct ZeroizingOutboundFrame {
	blocks: Vec<ZeroizingOutboundBlock>,
	len: usize,
	limit_exceeded: bool,
}
impl ZeroizingOutboundFrame {
	fn new() -> Self {
		Self { blocks: Vec::new(), len: 0, limit_exceeded: false }
	}

	fn serialize<T>(value: &T) -> Result<Self, ProbeError>
	where
		T: Serialize + ?Sized,
	{
		let mut frame = Self::new();

		if serde_json::to_writer(&mut frame, value).is_err() || frame.write_all(b"\n").is_err() {
			return Err(if frame.limit_exceeded {
				SupervisionError::ProtocolLimitExceeded.into()
			} else {
				SupervisionError::WriteFailed.into()
			});
		}

		Ok(frame)
	}

	fn chunks(&self) -> impl Iterator<Item = &[u8]> {
		let mut remaining = self.len;

		self.blocks.iter().map(move |block| {
			let count = remaining.min(OUTBOUND_BLOCK_BYTES);

			remaining -= count;

			&block.bytes[..count]
		})
	}

	fn sha256(&self) -> String {
		let mut digest = Sha256::new();

		for chunk in self.chunks() {
			digest.update(chunk);
		}

		hex_digest(&digest.finalize())
	}

	fn write_to(&self, writer: &mut impl Write) -> Result<(), ProbeError> {
		for chunk in self.chunks() {
			writer
				.write_all(chunk)
				.map_err(|_| ProbeError::Supervision(SupervisionError::WriteFailed))?;
		}

		Ok(())
	}
}
impl Write for ZeroizingOutboundFrame {
	fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
		if self.len.saturating_add(bytes.len()) > MAX_APP_SERVER_FRAME_BYTES {
			self.limit_exceeded = true;

			return Err(io::Error::new(ErrorKind::FileTooLarge, "outbound frame limit"));
		}

		let written = bytes.len();

		while !bytes.is_empty() {
			let offset = self.len % OUTBOUND_BLOCK_BYTES;

			if offset == 0 {
				self.blocks.push(ZeroizingOutboundBlock::new());
			}

			let count = bytes.len().min(OUTBOUND_BLOCK_BYTES - offset);
			let block = self.blocks.last_mut().expect("an outbound block was just installed");

			block.bytes[offset..offset + count].copy_from_slice(&bytes[..count]);

			self.len += count;
			bytes = &bytes[count..];
		}

		Ok(written)
	}

	fn flush(&mut self) -> io::Result<()> {
		Ok(())
	}
}

fn rpc_supervision(error: ProbeError) -> RpcError {
	match error {
		ProbeError::Supervision(error) => RpcError::Supervision(error),
		_ => RpcError::Supervision(SupervisionError::InvalidProtocol),
	}
}

fn resolve_executable(
	program: &OsStr,
) -> Result<(PathBuf, Arc<ExecutableSnapshot>, [u8; 32]), SupervisionError> {
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
	let (snapshot, digest) = capture_executable_snapshot(&canonical)?;

	Ok((canonical, Arc::new(snapshot), digest))
}

fn capture_executable_snapshot(
	path: &Path,
) -> Result<(ExecutableSnapshot, [u8; 32]), SupervisionError> {
	let source = File::open(path).map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let metadata = source.metadata().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	validate_executable_metadata(&metadata)?;
	validate_native_executable(&source)?;

	capture_platform_executable_snapshot(&source, &metadata)
}

#[cfg(target_os = "macos")]
fn capture_platform_executable_snapshot(
	source: &File,
	metadata: &Metadata,
) -> Result<(ExecutableSnapshot, [u8; 32]), SupervisionError> {
	let directory = TempDir::new().map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let snapshot_path = directory.path().join("verified-codex-image");
	let mut snapshot_writer = OpenOptions::new()
		.create_new(true)
		.write(true)
		.mode(0o500)
		.open(&snapshot_path)
		.map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let mut offset = 0_u64;
	let mut buffer = [0_u8; 64 * 1_024];

	loop {
		let count = source
			.read_at(&mut buffer, offset)
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;

		if count == 0 {
			break;
		}

		offset = offset
			.checked_add(count as u64)
			.filter(|offset| *offset <= MAX_EXECUTABLE_BYTES)
			.ok_or(SupervisionError::ExecutableUnavailable)?;

		snapshot_writer
			.write_all(&buffer[..count])
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;
	}

	snapshot_writer.sync_all().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	drop(snapshot_writer);

	fs::set_permissions(&snapshot_path, Permissions::from_mode(0o500))
		.map_err(|_| SupervisionError::ExecutableUnavailable)?;

	set_snapshot_immutable(&snapshot_path, true)?;

	let digest = executable_digest(&snapshot_path)?;
	let snapshot = ExecutableSnapshot {
		_directory: directory,
		path: snapshot_path,
		source_device: metadata.dev(),
		source_inode: metadata.ino(),
	};

	Ok((snapshot, digest))
}
#[cfg(target_os = "linux")]
fn capture_platform_executable_snapshot(
	source: &File,
	metadata: &Metadata,
) -> Result<(ExecutableSnapshot, [u8; 32]), SupervisionError> {
	let name = c"decodex-codex-image";
	let flags = MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_EXEC;
	// SAFETY: `name` is a static NUL-terminated string and the flags request an executable,
	// close-on-exec memfd whose contents can be sealed before any child exists.
	let descriptor = unsafe { libc::memfd_create(name.as_ptr(), flags) };

	if descriptor == -1 {
		return Err(SupervisionError::ExecutableUnavailable);
	}

	// SAFETY: `memfd_create` returned a new descriptor and ownership moves into this `File` once.
	let mut snapshot = unsafe { File::from_raw_fd(descriptor) };
	let mut offset = 0_u64;
	let mut buffer = [0_u8; 64 * 1_024];

	loop {
		let count = source
			.read_at(&mut buffer, offset)
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;

		if count == 0 {
			break;
		}

		offset = offset
			.checked_add(count as u64)
			.filter(|offset| *offset <= MAX_EXECUTABLE_BYTES)
			.ok_or(SupervisionError::ExecutableUnavailable)?;

		snapshot
			.write_all(&buffer[..count])
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;
	}

	snapshot.sync_all().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	// SAFETY: the descriptor is owned and the requested mode retains owner execution only.
	if unsafe { libc::fchmod(snapshot.as_raw_fd(), 0o500) } == -1 {
		return Err(SupervisionError::ExecutableUnavailable);
	}

	let required_seals = F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_EXEC | F_SEAL_SEAL;

	// SAFETY: F_ADD_SEALS applies atomically to this owned seal-capable memfd.
	if unsafe { libc::fcntl(snapshot.as_raw_fd(), F_ADD_SEALS, required_seals) } == -1 {
		return Err(SupervisionError::ExecutableUnavailable);
	}

	// SAFETY: F_GET_SEALS has no variadic argument and only reads inode-wide seal state.
	let observed_seals = unsafe { libc::fcntl(snapshot.as_raw_fd(), F_GET_SEALS) };

	if observed_seals == -1 || observed_seals & required_seals != required_seals {
		return Err(SupervisionError::ExecutableUnavailable);
	}

	let execution_path = linux_execution_path(&snapshot);
	let path_metadata =
		execution_path.metadata().map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let snapshot_metadata =
		snapshot.metadata().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	if path_metadata.dev() != snapshot_metadata.dev()
		|| path_metadata.ino() != snapshot_metadata.ino()
	{
		return Err(SupervisionError::ExecutableUnavailable);
	}

	let digest = executable_digest_file(&snapshot)?;
	let snapshot = ExecutableSnapshot {
		file: snapshot,
		source_device: metadata.dev(),
		source_inode: metadata.ino(),
	};

	Ok((snapshot, digest))
}

#[cfg(target_os = "linux")]
fn linux_execution_path(file: &File) -> PathBuf {
	PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

fn validate_executable_metadata(metadata: &Metadata) -> Result<(), SupervisionError> {
	if !metadata.is_file()
		|| metadata.permissions().mode() & 0o111 == 0
		|| metadata.len() > MAX_EXECUTABLE_BYTES
	{
		return Err(SupervisionError::ExecutableUnavailable);
	}

	Ok(())
}

fn validate_native_executable(file: &File) -> Result<(), SupervisionError> {
	let mut magic = [0_u8; 4];

	if file.read_at(&mut magic, 0).map_err(|_| SupervisionError::ExecutableUnavailable)?
		!= magic.len()
		|| !is_supported_native_executable_magic(magic)
	{
		return Err(SupervisionError::ExecutableUnavailable);
	}

	Ok(())
}

#[cfg(target_os = "macos")]
const fn is_supported_native_executable_magic(magic: [u8; 4]) -> bool {
	matches!(
		magic,
		// Thin 64-bit Mach-O, in native or swapped byte order. Current supported macOS
		// hosts do not execute legacy thin 32-bit images.
		[0xfe, 0xed, 0xfa, 0xcf]
			| [0xcf, 0xfa, 0xed, 0xfe]
			// Universal 32/64-bit Mach-O containers, in native or swapped byte order.
			| [0xca, 0xfe, 0xba, 0xbe]
			| [0xbe, 0xba, 0xfe, 0xca]
			| [0xca, 0xfe, 0xba, 0xbf]
			| [0xbf, 0xba, 0xfe, 0xca]
	)
}

#[cfg(target_os = "linux")]
const fn is_supported_native_executable_magic(magic: [u8; 4]) -> bool {
	matches!(magic, [0x7f, b'E', b'L', b'F'])
}

#[cfg(target_os = "macos")]
fn set_snapshot_immutable(path: &Path, immutable: bool) -> Result<(), SupervisionError> {
	let path = CString::new(path.as_os_str().as_bytes())
		.map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let flags = if immutable { UF_IMMUTABLE } else { 0 };
	// SAFETY: the C string is NUL-terminated and remains alive for the call.
	let result = unsafe { libc::chflags(path.as_ptr(), flags) };

	if result == 0 { Ok(()) } else { Err(SupervisionError::ExecutableUnavailable) }
}

fn executable_digest(path: &Path) -> Result<[u8; 32], SupervisionError> {
	let metadata = path.symlink_metadata().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	if metadata.file_type().is_symlink() {
		return Err(SupervisionError::ExecutableUnavailable);
	}

	validate_executable_metadata(&metadata)?;

	let file = File::open(path).map_err(|_| SupervisionError::ExecutableUnavailable)?;

	executable_digest_file(&file)
}

fn executable_digest_file(file: &File) -> Result<[u8; 32], SupervisionError> {
	let metadata = file.metadata().map_err(|_| SupervisionError::ExecutableUnavailable)?;

	validate_executable_metadata(&metadata)?;
	validate_native_executable(file)?;

	let mut hasher = Sha256::new();
	let mut remaining = MAX_EXECUTABLE_BYTES + 1;
	let mut buffer = [0_u8; 64 * 1_024];

	while remaining > 0 {
		let limit = usize::try_from(remaining.min(buffer.len() as u64))
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;
		let offset = MAX_EXECUTABLE_BYTES + 1 - remaining;
		let count = file
			.read_at(&mut buffer[..limit], offset)
			.map_err(|_| SupervisionError::ExecutableUnavailable)?;

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

fn verify_canonical_executable_identity(
	command: &AppServerCommand,
) -> Result<(), SupervisionError> {
	let canonical =
		command.program.canonicalize().map_err(|_| SupervisionError::ExecutableChanged)?;
	let source = canonical.metadata().map_err(|_| SupervisionError::ExecutableChanged)?;

	if canonical != command.program
		|| source.dev() != command.executable.source_device
		|| source.ino() != command.executable.source_inode
	{
		return Err(SupervisionError::ExecutableChanged);
	}

	Ok(())
}

fn verify_canonical_executable(command: &AppServerCommand) -> Result<(), SupervisionError> {
	verify_canonical_executable_identity(command)?;
	if executable_digest(&command.program).map_err(|_| SupervisionError::ExecutableChanged)?
		!= command.executable_digest
	{
		return Err(SupervisionError::ExecutableChanged);
	}

	Ok(())
}

fn verify_executable(command: &AppServerCommand) -> Result<(), SupervisionError> {
	verify_canonical_executable(command)?;
	if command.executable.digest().map_err(|_| SupervisionError::ExecutableChanged)?
		!= command.executable_digest
	{
		return Err(SupervisionError::ExecutableChanged);
	}

	Ok(())
}

fn run_before_spawn_test(_command: &AppServerCommand) {
	#[cfg(test)]
	if let Some(control) = &_command.before_spawn_test {
		let spawn = control.spawn_count.fetch_add(1, Ordering::AcqRel) + 1;

		if spawn == control.trigger_spawn {
			(control.action)();
		}
	}
}

fn run_after_verification_test(_command: &AppServerCommand) {
	#[cfg(test)]
	if let Some(control) = &_command.after_verification_test {
		let spawn = control.spawn_count.fetch_add(1, Ordering::AcqRel) + 1;

		if spawn == control.trigger_spawn {
			(control.action)();
		}
	}
}

#[cfg(not(target_os = "macos"))]
fn configured_app_server_process(
	command: &AppServerCommand,
	binding: &AccountBinding,
) -> Result<Command, SupervisionError> {
	verify_executable(command)?;
	run_before_spawn_test(command);
	verify_executable(command)?;
	run_after_verification_test(command);

	let mut process = Command::new(command.executable.execution_path());
	process
		.arg0(&command.program)
		.args(&command.app_server_args)
		.current_dir(&command.working_directory)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null());
	configure_child_environment(&mut process, binding)?;
	configure_process_session(&mut process, None);

	Ok(process)
}

#[cfg(not(target_os = "macos"))]
fn configured_attested_app_server_process(
	command: &AppServerCommand,
	binding: &AccountBinding,
	capability: ExactBuildLaunchCapability,
) -> Result<Command, SupervisionError> {
	let mut process = configured_app_server_process(command, binding)?;
	match capability {
		ExactBuildLaunchCapability::PrivateStdioDisabledEphemeralStartupV1 => {
			process.env(PRIVATE_STDIO_STARTUP_ENV, PRIVATE_STDIO_STARTUP_VALUE);
		},
	}
	capability.lifetime().configure(&mut process)?;
	Ok(process)
}

fn attested_launch_identity(
	command: &AppServerCommand,
	binding: &AccountBinding,
	build: &BuildId,
	capability: ExactBuildLaunchCapability,
) -> Result<ProcessRunnerIdentity, SupervisionError> {
	let home = binding.expected_codex_home.parent().ok_or(SupervisionError::InvalidBinding)?;
	let mut digest = Sha256::new();

	hash_launch_field(&mut digest, b"schema", b"decodex/attested-app-server-launch/1");
	hash_launch_field(&mut digest, b"exec-policy", b"macos-canonical-suspended-v1");
	hash_launch_field(&mut digest, b"platform", STDIO_ONLY_ATTESTED_PLATFORM.as_bytes());
	hash_launch_field(
		&mut digest,
		b"control-kind",
		capability.lifetime().control_kind().as_sql().as_bytes(),
	);
	hash_launch_field(
		&mut digest,
		b"isolation-kind",
		ProcessIsolationKind::Session.as_sql().as_bytes(),
	);
	hash_launch_field(&mut digest, b"build", build.as_str().as_bytes());
	hash_launch_field(&mut digest, b"image", &command.executable_digest);
	hash_launch_field(&mut digest, b"arg0", command.program.as_os_str().as_bytes());
	for argument in &command.app_server_args {
		hash_launch_field(&mut digest, b"argument", argument.as_os_str().as_bytes());
	}
	hash_launch_field(
		&mut digest,
		b"working-directory",
		command.working_directory.as_os_str().as_bytes(),
	);
	hash_launch_field(&mut digest, b"environment-policy", b"clear-then-set-v1");
	hash_launch_field(&mut digest, b"environment-name", b"HOME");
	hash_launch_field(&mut digest, b"environment-value", home.as_os_str().as_bytes());
	hash_launch_field(&mut digest, b"environment-name", b"PATH");
	hash_launch_field(&mut digest, b"environment-value", CHILD_PATH.as_bytes());
	hash_launch_field(&mut digest, b"environment-name", PRIVATE_STDIO_STARTUP_ENV.as_bytes());
	hash_launch_field(&mut digest, b"environment-value", PRIVATE_STDIO_STARTUP_VALUE.as_bytes());
	hash_launch_field(&mut digest, b"account", binding.account_id.as_str().as_bytes());
	let process_binding = binding.process_binding()?;
	hash_launch_field(
		&mut digest,
		b"account-revision",
		process_binding.account_revision.to_string().as_bytes(),
	);
	hash_launch_field(
		&mut digest,
		b"credential-store-schema",
		process_binding.credential.schema_version.get().to_string().as_bytes(),
	);
	hash_launch_field(
		&mut digest,
		b"credential-version",
		process_binding.credential.version.get().to_string().as_bytes(),
	);
	hash_launch_field(
		&mut digest,
		b"credential-fingerprint",
		process_binding.credential.fingerprint.as_str().as_bytes(),
	);
	hash_launch_field(
		&mut digest,
		b"credential-writer-operation",
		process_binding.credential.writer_operation_id.as_str().as_bytes(),
	);
	hash_launch_field(&mut digest, b"provider-kind", b"chatgpt");
	hash_launch_field(
		&mut digest,
		b"provider-account",
		process_binding.credential.provider.account_id().as_bytes(),
	);
	hash_launch_field(
		&mut digest,
		b"refresh-callback-profile",
		process_binding.refresh_callback_profile_sha256.as_bytes(),
	);
	hash_launch_field(&mut digest, b"capability", capability.identity().as_bytes());

	ProcessRunnerIdentity::new(format!("sha256:{}", hex_digest(&digest.finalize())))
		.map_err(|_| SupervisionError::LaunchCapabilityUnavailable)
}

fn hash_launch_field(hasher: &mut Sha256, label: &[u8], value: &[u8]) {
	hasher.update((label.len() as u64).to_be_bytes());
	hasher.update(label);
	hasher.update((value.len() as u64).to_be_bytes());
	hasher.update(value);
}

fn cleanup_process_group_once(job: &mut ReapJob) -> bool {
	#[cfg(test)]
	if job.not_before.is_some_and(|not_before| Instant::now() < not_before) {
		return false;
	}

	let exited = match job.child.try_wait() {
		Ok(Some(_)) => {
			job.may_signal_process_group = false;
			true
		},
		Ok(None) => false,
		Err(_) => {
			job.may_signal_process_group = false;
			false
		},
	};
	let group_absent = matches!(process_group_exists(job.process_group), Ok(false));

	if exited && group_absent {
		let pump_stopped = job.pump.as_mut().is_none_or(|pump| pump.stop(Duration::ZERO));

		if pump_stopped {
			job.pump = None;

			return true;
		}
	}

	if job.may_signal_process_group {
		let _ = signal_process_group(job.process_group, SIGKILL);
	}

	false
}

fn account_identity(response: AccountReadResponse) -> Result<AccountIdentity, ProbeError> {
	let account = response.account.as_ref().ok_or(SupervisionError::AccountUnavailable)?;

	Ok(AccountIdentity::from_observation(
		account.kind.as_str(),
		account.email.as_deref(),
		response.requires_openai_auth,
	))
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
	vault: Option<&dyn CredentialVault>,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<AccountIdentity, ProbeError> {
	if let Some(vault) = vault {
		initialize_probe_projection(process, vault, timeout, negotiation)?;
	} else {
		initialize_probe_connection(process, timeout, negotiation)?;
	}

	let identity = match process.read_account_identity(timeout) {
		Ok(identity) => identity,
		Err(error) => return negotiation.fail(Capability::AccountRead, error),
	};

	negotiation.observe(Capability::AccountRead, LiveMethodOutcome::Supported);

	Ok(identity)
}

fn initialize_probe_projection(
	process: &mut SupervisedProcess,
	vault: &dyn CredentialVault,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<(), ProbeError> {
	initialize_probe_connection(process, timeout, negotiation)?;
	let account_id = process.binding.account_id().clone();
	let mut projection = CredentialProjection { process, timeout, used: false };
	let expected =
		vault.project(&account_id, &mut projection).map_err(ProbeError::CredentialVault)?;

	if !projection.used {
		return Err(ProbeError::CredentialVault(CredentialVaultError::Unavailable));
	}

	projection.process.expected_account_identity = Some(expected);

	Ok(())
}

fn initialize_probe_connection(
	process: &mut SupervisedProcess,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<(), ProbeError> {
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

	Ok(())
}

fn probe_thread_list(
	process: &mut SupervisedProcess,
	timeout: Duration,
	negotiation: &mut ProbeNegotiation<'_>,
) -> Result<ThreadListResponse, ProbeError> {
	let list = match process.request::<_, ThreadListResponse>(
		ReadOnlyMethod::ThreadList,
		&ThreadListParams {
			search_term: THREAD_LIST_PROBE_SEARCH_TERM,
			limit: THREAD_LIST_LIMIT as u32,
			use_state_db_only: true,
		},
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
		&ThreadReadParams { thread_id: thread.id.as_str(), include_turns: false },
		timeout,
	);
	let terminal_error = match result {
		Ok(response) if response.thread.id.as_str() == thread.id.as_str() => {
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

fn send_inbound_frame(
	sender: &SyncSender<InboundFrame>,
	frame: InboundFrame,
	protocol_limit_exceeded: &AtomicBool,
) -> bool {
	match sender.try_send(frame) {
		Ok(()) => true,
		Err(TrySendError::Full(_)) => {
			protocol_limit_exceeded.store(true, Ordering::Release);

			false
		},
		Err(TrySendError::Disconnected(_)) => false,
	}
}

fn set_nonblocking(descriptor: i32) -> Result<(), SupervisionError> {
	let flags = unsafe { libc::fcntl(descriptor, F_GETFL) };

	if flags == -1 || unsafe { libc::fcntl(descriptor, F_SETFL, flags | O_NONBLOCK) } == -1 {
		return Err(SupervisionError::InvalidProtocol);
	}

	Ok(())
}

fn pump_stdout(
	mut reader: impl Read,
	sender: SyncSender<InboundFrame>,
	protocol_limit_exceeded: Arc<AtomicBool>,
	cancelled: &AtomicBool,
	buffered: Option<&AtomicBool>,
) {
	let mut read_buffer = ZeroizingInboundBlock::new();
	let mut frame = InboundFrame::new();

	loop {
		if cancelled.load(Ordering::Acquire) {
			break;
		}

		read_buffer.wipe();

		let count = match reader.read(&mut read_buffer.bytes) {
			Ok(0) => {
				if !frame.is_empty() {
					let _ = send_inbound_frame(&sender, frame, &protocol_limit_exceeded);
				}

				break;
			},
			Ok(count) => count,
			Err(error) if error.kind() == ErrorKind::WouldBlock => {
				thread::park_timeout(Duration::from_millis(10));

				continue;
			},
			Err(_) => break,
		};
		let mut unread = &read_buffer.bytes[..count];

		while !unread.is_empty() {
			let consumed =
				unread.iter().position(|byte| *byte == b'\n').map_or(unread.len(), |at| at + 1);

			if frame.extend_from_slice(&unread[..consumed]).is_err() {
				protocol_limit_exceeded.store(true, Ordering::Release);

				return;
			}

			if let Some(buffered) = buffered {
				buffered.store(true, Ordering::Release);
			}

			unread = &unread[consumed..];

			if frame.len > 0
				&& frame.blocks.last().is_some_and(|block| {
					block.bytes[(frame.len - 1) % INBOUND_BLOCK_BYTES] == b'\n'
				}) {
				let complete = mem::replace(&mut frame, InboundFrame::new());

				if !send_inbound_frame(&sender, complete, &protocol_limit_exceeded) {
					return;
				}
			}
		}
	}
}

fn attest_executable(
	command: &AppServerCommand,
	binding: &AccountBinding,
	timeout: Duration,
	guard: Option<RunnerPermit>,
) -> Result<(BuildId, GeneratedSchemaEvidence, Option<RunnerPermit>), ProbeError> {
	attest_executable_for_home(command, &binding.expected_codex_home, timeout, guard)
}

fn attest_executable_for_home(
	command: &AppServerCommand,
	expected_codex_home: &Path,
	timeout: Duration,
	guard: Option<RunnerPermit>,
) -> Result<(BuildId, GeneratedSchemaEvidence, Option<RunnerPermit>), ProbeError> {
	let deadline = Instant::now() + timeout;
	let version_output = NamedTempFile::new().map_err(|_| SupervisionError::PreflightFailed)?;
	let version_writer = version_output.reopen().map_err(|_| SupervisionError::PreflightFailed)?;
	let (version_status, guard) = run_preflight_command(
		command,
		expected_codex_home,
		&command.version_args,
		Stdio::from(version_writer),
		preflight_remaining(deadline)?,
		MAX_VERSION_OUTPUT_BYTES,
		guard,
	)?;

	if !version_status.success() {
		return Err(SupervisionError::PreflightFailed.into());
	}
	if version_output.as_file().metadata().map_err(|_| SupervisionError::PreflightFailed)?.len()
		> MAX_VERSION_OUTPUT_BYTES
	{
		return Err(SupervisionError::PreflightFailed.into());
	}

	let version = Zeroizing::new(
		fs::read(version_output.path()).map_err(|_| SupervisionError::PreflightFailed)?,
	);
	let version = str::from_utf8(&version).map_err(|_| SupervisionError::PreflightFailed)?;
	let build = BuildId::from_attestation(version.trim(), &command.executable_digest)
		.map_err(|_| SupervisionError::PreflightFailed)?;
	let schema_directory = TempDir::new().map_err(|_| SupervisionError::PreflightFailed)?;
	let mut schema_args = command.schema_args.clone();

	schema_args.push(schema_directory.path().as_os_str().to_owned());

	let (status, guard) = run_preflight_command(
		command,
		expected_codex_home,
		&schema_args,
		Stdio::null(),
		preflight_remaining(deadline)?,
		MAX_SCHEMA_FILE_BYTES,
		guard,
	)?;

	if !status.success() {
		return Err(SupervisionError::PreflightFailed.into());
	}

	let generated = GeneratedSchemaEvidence::load(schema_directory.path())
		.map_err(|markers| ProbeError::SchemaMissing { markers })?;

	Ok((build, generated, guard))
}

fn protected_spawn_path(command: &AppServerCommand) -> PathBuf {
	// macOS 27 terminates relocated Apple platform binaries such as /usr/bin/python3 before their
	// test fixture can start. Production version/schema preflights execute the protected snapshot;
	// the final macOS app-server instead uses canonical suspended dynamic attestation. Unit
	// fixtures execute their already-verified canonical interpreter while still exercising
	// snapshot capture, digest verification, immutability, and cleanup mechanics.
	#[cfg(all(test, target_os = "macos"))]
	return command.test_spawn_path.clone().unwrap_or_else(|| command.program.clone());

	#[cfg(not(all(test, target_os = "macos")))]
	command.executable.execution_path()
}

fn preflight_remaining(deadline: Instant) -> Result<Duration, SupervisionError> {
	let remaining = deadline.saturating_duration_since(Instant::now());

	if remaining.is_zero() { Err(SupervisionError::PreflightFailed) } else { Ok(remaining) }
}

fn run_preflight_command(
	command: &AppServerCommand,
	expected_codex_home: &Path,
	args: &[OsString],
	stdout: Stdio,
	timeout: Duration,
	max_file_bytes: u64,
	guard: Option<RunnerPermit>,
) -> Result<(ExitStatus, Option<RunnerPermit>), SupervisionError> {
	verify_executable(command)?;
	run_before_spawn_test(command);
	verify_executable(command)?;
	run_after_verification_test(command);

	let mut process = Command::new(protected_spawn_path(command));

	process
		.arg0(&command.program)
		.args(args)
		.current_dir(&command.working_directory)
		.stdin(Stdio::null())
		.stdout(stdout)
		.stderr(Stdio::null());

	configure_home_environment(&mut process, expected_codex_home)?;
	configure_process_session(&mut process, Some(max_file_bytes));

	let started = Instant::now();
	let term_deadline = started + timeout / 2;
	let kill_deadline = started + timeout * 3 / 4;
	let hard_deadline = started + timeout;
	let child = process.spawn().map_err(|_| SupervisionError::PreflightFailed)?;
	let mut owner = ProcessGroupOwner::new(child, guard);

	#[cfg(test)]
	if let Some(control) = &command.preflight_cleanup_test {
		owner
			.guard
			.as_mut()
			.expect("test preflight owns capacity")
			.use_quarantine_for_test(&control.quarantine);

		let spawn = control.spawn_count.fetch_add(1, Ordering::AcqRel) + 1;

		if spawn == control.trigger_spawn {
			owner.reap_not_before = Some(Instant::now() + control.reaper_delay);

			control.process_group.store(owner.process_id(), Ordering::Release);
			owner.transfer_to_reaper();

			return Err(SupervisionError::PreflightFailed);
		}
	}

	let mut status = None;
	let mut term_sent = false;
	let mut kill_sent = false;

	loop {
		if status.is_none() {
			status = owner.try_wait().map_err(|_| SupervisionError::PreflightFailed)?;
		}

		let group_exists = process_group_exists(owner.process_id())?;

		if let Some(status) = status
			&& !group_exists
		{
			let (_, guard) = owner.shutdown_retaining_guard(Duration::from_millis(1))?;

			return Ok((status, guard));
		}

		let now = Instant::now();

		if owner.may_signal_process_group() && !term_sent && now >= term_deadline {
			signal_process_group(owner.process_id(), SIGTERM)?;

			term_sent = true;
		}
		if owner.may_signal_process_group() && !kill_sent && now >= kill_deadline {
			signal_process_group(owner.process_id(), SIGKILL)?;

			kill_sent = true;
		}
		if now >= hard_deadline {
			return Err(SupervisionError::PreflightFailed);
		}

		thread::sleep(Duration::from_millis(10));
	}
}

fn configure_child_environment(
	command: &mut Command,
	binding: &AccountBinding,
) -> Result<(), SupervisionError> {
	configure_home_environment(command, &binding.expected_codex_home)
}

fn configure_home_environment(
	command: &mut Command,
	expected_codex_home: &Path,
) -> Result<(), SupervisionError> {
	let home = expected_codex_home.parent().ok_or(SupervisionError::InvalidBinding)?;

	command.env_clear().env("HOME", home).env("PATH", CHILD_PATH);

	Ok(())
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
	if binding.expected_codex_home.as_os_str() != OsStr::new(value.codex_home.as_str()) {
		return Err(ProbeError::Supervision(SupervisionError::CodexHomeMismatch));
	}

	Ok(())
}

fn terminate_process_group(
	child: &mut ManagedChild,
	pid: u32,
	timeout: Duration,
	may_signal_process_group: &mut bool,
) -> Result<ShutdownOutcome, SupervisionError> {
	let leader_exited = observe_owned_child_exit(child, may_signal_process_group)?;

	if !process_group_exists(pid)? {
		return Ok(ShutdownOutcome::Exited);
	}

	if *may_signal_process_group {
		signal_process_group(pid, SIGTERM)?;
	}

	let started = Instant::now();
	let term_deadline = started + timeout / 2;
	let hard_deadline = started + timeout;

	while Instant::now() < term_deadline {
		let _ = observe_owned_child_exit(child, may_signal_process_group)?;

		if !process_group_exists(pid)? {
			return Ok(if leader_exited {
				ShutdownOutcome::Exited
			} else {
				ShutdownOutcome::Terminated
			});
		}

		thread::sleep(Duration::from_millis(10));
	}

	if *may_signal_process_group {
		signal_process_group(pid, SIGKILL)?;
	}

	while Instant::now() < hard_deadline {
		let _ = observe_owned_child_exit(child, may_signal_process_group)?;

		if !process_group_exists(pid)? {
			return Ok(ShutdownOutcome::KilledAfterTimeout);
		}

		thread::sleep(Duration::from_millis(10));
	}

	Err(SupervisionError::ShutdownFailed)
}

fn observe_owned_child_exit(
	child: &mut ManagedChild,
	may_signal_process_group: &mut bool,
) -> Result<bool, SupervisionError> {
	match child.try_wait() {
		Ok(Some(_)) => {
			*may_signal_process_group = false;
			Ok(true)
		},
		Ok(None) => Ok(false),
		Err(_) => {
			*may_signal_process_group = false;
			Err(SupervisionError::ShutdownFailed)
		},
	}
}

#[cfg(unix)]
fn configure_process_session(command: &mut Command, max_file_bytes: Option<u64>) {
	crate::process_platform::configure_session_command(command, max_file_bytes);
}

#[cfg(not(unix))]
fn configure_process_session(_command: &mut Command, _max_file_bytes: Option<u64>) {}

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
		env,
		ffi::{OsStr, OsString},
		fs,
		io::{self, Cursor, ErrorKind, Write},
		mem,
		os::unix::fs::{MetadataExt as _, PermissionsExt as _},
		path::{Path, PathBuf},
		process::{Command, Stdio},
		sync::{
			Arc,
			atomic::{AtomicBool, AtomicU32, Ordering},
			mpsc,
		},
		thread,
		time::{Duration, Instant},
	};

	use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
	use serde::{
		Serialize,
		ser::{Error as _, SerializeMap as _},
	};
	use serde_json::Value;
	use tempfile::TempDir;

	use crate::account_launch::{
		RunnerCapacity, RunnerPermit,
		process::{
			self, AccountBinding, AccountIdentity, AccountRefreshCallback, AppServerCommand,
			AttestedAppServerLaunch, AttestedAppServerProfile, AttestedProcessChild,
			CALLBACK_PROBE_EMAIL, CALLBACK_PROBE_PLAN_TYPE, CALLBACK_PROBE_SIGNATURE,
			ChatgptRefreshProjection, CredentialProjection, CredentialProjectionResponse,
			CredentialVault, CredentialVaultError, ExactThreadReconciler,
			ExactThreadReconciliation, ExactThreadReconciliationResult, PROTOCOL_QUEUE_CAPACITY,
			ProbeError, ProcessQuarantine, ReadOnlyMethod, ReadOnlyProbe, ResetCardProcessError,
			ResetCardProcessMethod, ResetCardProcessRunner, RpcError, ShutdownOutcome,
			SupervisedProcess, SupervisionError, UnavailableCredentialVault,
			callback_probe_access_token,
		},
		protocol::{
			self, ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse,
			JsonRpcResponse,
		},
	};
	use decodex_codex::{
		ArchiveReconciliationOutcome, ArchiveUnverifiedReason, Capability, CapabilityCache,
		CapabilityState, DecodexThreadSearchTerm, ExactResetCreditId, ExactThreadId,
		ExactThreadListFilter, ResetCardCapabilityState, ResetCardIdempotencyKey, SchemaMarker,
		ThreadArchivedFilter, UnavailableReason, UnsupportedReason,
	};
	use decodex_core::{
		AccountId, AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
		CredentialStoreSchemaVersion, CredentialVersion, ProcessGenerationAccountBinding,
		ProviderIdentity, ResetCardConsumeOutcome,
	};

	struct TestCapacity {
		inner: RunnerCapacity,
	}
	impl TestCapacity {
		fn new(limit: u16) -> Self {
			Self { inner: RunnerCapacity::try_with_limit(limit).unwrap() }
		}

		fn reserve(&self) -> Result<RunnerPermit, ()> {
			self.inner
				.reserve(AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(), 1)
				.map_err(|_| ())
		}

		fn active(&self) -> u16 {
			self.inner.active()
		}
	}

	struct FixtureVault {
		expected_email: &'static str,
		process_id: AtomicU32,
		double_project: bool,
	}

	impl FixtureVault {
		fn matching() -> Self {
			Self {
				expected_email: "private@example.test",
				process_id: AtomicU32::new(0),
				double_project: false,
			}
		}
	}

	impl CredentialVault for FixtureVault {
		fn project(
			&self,
			account_id: &AccountId,
			projection: &mut CredentialProjection<'_>,
		) -> Result<AccountIdentity, CredentialVaultError> {
			assert_eq!(account_id, binding().account_id());

			self.process_id.store(projection.process.process_id(), Ordering::Release);
			projection.authenticate_chatgpt(
				"synthetic-nonsecret-sentinel",
				"synthetic-provider-sentinel",
				Some("synthetic-plan"),
			)?;

			if self.double_project {
				projection.authenticate_chatgpt(
					"synthetic-nonsecret-sentinel",
					"synthetic-provider-sentinel",
					Some("synthetic-plan"),
				)?;
			}

			Ok(AccountIdentity::from_observation("chatgpt", Some(self.expected_email), true))
		}
	}

	struct CallbackProbeFixtureVault;
	impl CredentialVault for CallbackProbeFixtureVault {
		fn project(
			&self,
			account_id: &AccountId,
			projection: &mut CredentialProjection<'_>,
		) -> Result<AccountIdentity, CredentialVaultError> {
			assert_eq!(account_id, binding().account_id());
			projection.authenticate_callback_probe("callback-provider-account")?;
			Ok(AccountIdentity::from_observation("chatgpt", Some("private@example.test"), true))
		}
	}

	#[derive(Default)]
	struct FixtureRefreshCallback {
		calls: AtomicU32,
	}
	impl AccountRefreshCallback for FixtureRefreshCallback {
		fn refresh(
			&self,
			account_id: &AccountId,
			initial_binding: &ProcessGenerationAccountBinding,
			reason: &str,
			previous_provider_account_id: Option<&str>,
		) -> Result<ChatgptRefreshProjection, CredentialVaultError> {
			assert_eq!(account_id, binding().account_id());
			assert_eq!(
				initial_binding.credential.provider.account_id(),
				"callback-provider-account"
			);
			assert_eq!(reason, "unauthorized");
			assert_eq!(previous_provider_account_id, Some("callback-provider-account"));
			self.calls.fetch_add(1, Ordering::AcqRel);
			ChatgptRefreshProjection::new(
				"synthetic-successor-token".to_owned(),
				"callback-provider-account".to_owned(),
				Some("business".to_owned()),
			)
		}
	}

	fn profile_binding(
		account_id: AccountId,
		account_revision: i64,
		callback_profile_sha256: String,
	) -> AccountBinding {
		let credential = CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(1).unwrap(),
			fingerprint: CredentialFingerprint::new("1".repeat(64)).unwrap(),
			provider: ProviderIdentity::new(
				AccountProvider::Chatgpt,
				format!("provider-{account_revision}"),
			)
			.unwrap(),
			writer_operation_id: AccountOperationId::new("20000000-0000-4000-8000-000000000001")
				.unwrap(),
		};
		AccountBinding {
			account_id,
			expected_codex_home: PathBuf::from("/tmp/.codex"),
			process_binding: Some(
				ProcessGenerationAccountBinding::new(
					account_revision,
					credential,
					callback_profile_sha256,
				)
				.unwrap(),
			),
			refresh_callback: Some(Arc::new(FixtureRefreshCallback::default())),
		}
	}

	fn callback_binding(callback: Arc<dyn AccountRefreshCallback>) -> AccountBinding {
		let credential = CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(1).unwrap(),
			fingerprint: CredentialFingerprint::new("1".repeat(64)).unwrap(),
			provider: ProviderIdentity::new(AccountProvider::Chatgpt, "callback-provider-account")
				.unwrap(),
			writer_operation_id: AccountOperationId::new("20000000-0000-4000-8000-000000000001")
				.unwrap(),
		};
		AccountBinding {
			account_id: binding().account_id().clone(),
			expected_codex_home: PathBuf::from("/tmp/.codex"),
			process_binding: Some(
				ProcessGenerationAccountBinding::new(1, credential, "2".repeat(64)).unwrap(),
			),
			refresh_callback: Some(callback),
		}
	}

	#[test]
	fn daemon_profile_reuses_one_snapshot_and_preflight_across_account_bindings() {
		let temp = TempDir::new().unwrap();
		let preflight_count = Arc::new(AtomicU32::new(0));
		let command = fake_command("reset-card", temp.path(), None).with_before_spawn_for_test(
			u32::MAX,
			Arc::clone(&preflight_count),
			Arc::new(|| {}),
		);
		let profile = AttestedAppServerProfile::attest_for_test(
			command,
			Path::new("/tmp/.codex"),
			Duration::from_secs(5),
		)
		.unwrap();
		let snapshot = Arc::clone(&profile.command.executable);
		let callback_profile = profile.generated.account_callback_profile_sha256().to_owned();
		let capacity = RunnerCapacity::try_with_limit(2).unwrap();
		let first_account = AccountId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let second_account = AccountId::new("10000000-0000-4000-8000-000000000002").unwrap();
		let first = AttestedAppServerLaunch::bind(
			profile.clone(),
			profile_binding(first_account.clone(), 1, callback_profile.clone()),
			Duration::from_secs(5),
			capacity.reserve(first_account, 1).unwrap(),
		)
		.unwrap();
		let second = AttestedAppServerLaunch::bind(
			profile.clone(),
			profile_binding(second_account.clone(), 2, callback_profile),
			Duration::from_secs(5),
			capacity.reserve(second_account, 2).unwrap(),
		)
		.unwrap();

		assert_eq!(preflight_count.load(Ordering::Acquire), 2);
		assert!(Arc::ptr_eq(&snapshot, &first.command.executable));
		assert!(Arc::ptr_eq(&snapshot, &second.command.executable));
		assert_ne!(first.runner_identity, second.runner_identity);
		assert_eq!(first.account_binding().account_revision, 1);
		assert_eq!(second.account_binding().account_revision, 2);
	}

	struct LateSerializationFailure<'a>(&'a str);
	impl Serialize for LateSerializationFailure<'_> {
		fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where
			S: serde::Serializer,
		{
			let mut map = serializer.serialize_map(Some(2))?;

			map.serialize_entry("accessToken", self.0)?;

			Err(S::Error::custom("synthetic late serialization failure"))
		}
	}

	struct FailAfterOneWrite(bool);
	impl Write for FailAfterOneWrite {
		fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
			if mem::replace(&mut self.0, true) {
				Err(io::Error::new(ErrorKind::BrokenPipe, "synthetic write failure"))
			} else {
				Ok(bytes.len())
			}
		}

		fn flush(&mut self) -> io::Result<()> {
			Ok(())
		}
	}

	fn fake_command(mode: &str, directory: &Path, extra: Option<&Path>) -> AppServerCommand {
		let fixture =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_app_server.py");
		let mut app_args =
			vec!["-B".into(), fixture.clone().into_os_string(), "serve".into(), mode.into()];

		if let Some(extra) = extra {
			app_args.push(extra.as_os_str().to_owned());
		}

		let mut schema_args = vec![
			"-B".into(),
			fixture.clone().into_os_string(),
			"generate-json-schema".into(),
			"--out".into(),
		];

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
		if mode == "nested-refresh-schema" {
			schema_args.push("--nested-refresh-only".into());
		}
		if mode == "preflight-orphan-error" {
			schema_args.push("--preflight-fail".into());
		}
		if mode == "preflight-uncertain-schema" {
			schema_args.push("--preflight-hang".into());
		}
		if matches!(
			mode,
			"reset-card"
				| "reset-card-partial-first"
				| "reset-card-missing-first"
				| "callback-probe"
		) {
			schema_args.push("--reset-card".into());
		}

		let version_flag = match mode {
			"preflight-hang" | "preflight-uncertain-version" => "--version-hang",
			"oversized-version" => "--version-oversized",
			_ => "--version",
		};

		AppServerCommand::new_for_test(
			"python3",
			app_args,
			["-B".into(), fixture.clone().into_os_string(), version_flag.into()],
			schema_args,
			directory,
		)
	}

	fn replaceable_fake_command(mode: &str, directory: &Path, source: &Path) -> AppServerCommand {
		let command = fake_command(mode, directory, None);
		#[cfg(target_os = "macos")]
		let spawn_path = command.program.clone();

		fs::copy(&command.program, source).unwrap();
		fs::set_permissions(source, fs::Permissions::from_mode(0o755)).unwrap();

		let command = AppServerCommand::new_for_test(
			source,
			command.app_server_args,
			command.version_args,
			command.schema_args,
			command.working_directory,
		);

		#[cfg(target_os = "macos")]
		let command = command.with_spawn_path_for_test(spawn_path);

		command
	}

	fn binding() -> AccountBinding {
		AccountBinding::for_test(PathBuf::from("/tmp/.codex"))
	}

	#[test]
	fn production_command_shape_cannot_inject_fake_launch_arguments() {
		let (_, executable, executable_digest) =
			process::resolve_executable(OsStr::new("python3")).unwrap();
		let command = AppServerCommand::production_from_resolved(
			PathBuf::from("/resolved/codex"),
			executable,
			executable_digest,
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
	fn native_executable_format_accepts_the_host_python_image() {
		let (program, _, digest) = process::resolve_executable(OsStr::new("python3")).unwrap();

		assert!(program.is_absolute());
		assert_ne!(digest, [0; 32]);
	}

	#[test]
	fn executable_scripts_are_rejected_before_a_probe_or_vault_can_exist() {
		let temp = TempDir::new().unwrap();

		for (name, script) in [
			("shell", "#!/bin/sh\nexit 0\n"),
			("python", "#!/usr/bin/env python3\nraise SystemExit(0)\n"),
			("node", "#!/usr/bin/env node\nprocess.exit(0)\n"),
		] {
			let path = temp.path().join(name);

			fs::write(&path, script).unwrap();
			fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

			assert!(matches!(
				process::resolve_executable(path.as_os_str()),
				Err(SupervisionError::ExecutableUnavailable)
			));
		}
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn native_executable_format_recognizes_thin_and_universal_mach_o_only() {
		for magic in [
			[0xfe, 0xed, 0xfa, 0xcf],
			[0xcf, 0xfa, 0xed, 0xfe],
			[0xca, 0xfe, 0xba, 0xbe],
			[0xbe, 0xba, 0xfe, 0xca],
			[0xca, 0xfe, 0xba, 0xbf],
			[0xbf, 0xba, 0xfe, 0xca],
		] {
			assert!(process::is_supported_native_executable_magic(magic));
		}
		for magic in [
			*b"#!/b",
			*b"{\"x\"",
			[0x7f, b'E', b'L', b'F'],
			[0xfe, 0xed, 0xfa, 0xce],
			[0xce, 0xfa, 0xed, 0xfe],
			[0; 4],
		] {
			assert!(!process::is_supported_native_executable_magic(magic));
		}
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn native_executable_format_recognizes_elf_only() {
		assert!(process::is_supported_native_executable_magic([0x7f, b'E', b'L', b'F']));

		for magic in
			[*b"#!/b", *b"{\"x\"", [0xfe, 0xed, 0xfa, 0xcf], [0xca, 0xfe, 0xba, 0xbe], [0; 4]]
		{
			assert!(!process::is_supported_native_executable_magic(magic));
		}
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
		assert_eq!(result.account_id, binding().account_id().clone());
		assert!(result.process_id > 0);
		assert!(result.profile.build().as_str().starts_with("sha256:"));
		assert_eq!(result.profile.build().as_str().len(), 71);
		assert_eq!(result.profile.schema_fingerprint().len(), 64);
		assert_eq!(
			result.profile.state(Capability::PaginatedHistory),
			&CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }
		);
		assert_eq!(
			result.profile.state(Capability::NativeCollaboration),
			&CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }
		);
	}

	#[test]
	fn app_server_envelope_accepts_legacy_v2_and_rejects_wrong_or_null_versions() {
		let temp = TempDir::new().unwrap();
		let result = ReadOnlyProbe::new_for_test(
			fake_command("legacy-jsonrpc", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default());

		assert!(result.is_ok(), "legacy JSON-RPC response failed: {result:?}");

		for mode in ["wrong-jsonrpc", "null-jsonrpc"] {
			let temp = TempDir::new().unwrap();
			let error = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(SupervisionError::InvalidProtocol));
		}
	}

	#[test]
	fn outbound_request_uses_the_native_bare_app_server_envelope() {
		let frame = process::exact_request_frame(
			7,
			"fixture/method",
			&serde_json::json!({"fixture": true}),
		)
		.unwrap();
		let mut bytes = Vec::new();

		frame.write_to(&mut bytes).unwrap();

		assert_eq!(
			bytes,
			br#"{"id":7,"method":"fixture/method","params":{"fixture":true}}
"#
		);
	}

	#[test]
	fn app_server_request_receives_bare_error_reply_before_probe_continues() {
		for mode in ["server-request", "server-request-id-collision"] {
			let temp = TempDir::new().unwrap();
			let result = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run(&mut CapabilityCache::default())
			.unwrap();

			assert_eq!(result.profile.state(Capability::Initialize), &CapabilityState::Supported);
		}
	}

	#[test]
	fn reset_card_runner_requires_both_exact_schema_methods_before_spawn() {
		let temp = TempDir::new().unwrap();
		let spawn_marker = temp.path().join("reset-card-spawned");
		let capacity = TestCapacity::new(1);
		let runner = ResetCardProcessRunner::new(
			fake_command("mark-spawn", temp.path(), Some(&spawn_marker)),
			binding(),
			Duration::from_secs(2),
		);

		let error = runner
			.read_inventory(&FixtureVault::matching(), capacity.reserve().unwrap())
			.unwrap_err();

		assert_eq!(
			error,
			ResetCardProcessError::SchemaUnsupported(
				ResetCardCapabilityState::ConsumeMethodMissing
			)
		);
		assert!(!spawn_marker.exists());
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn reset_card_runner_uses_null_read_params_and_completes_consume_readback() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let runner = ResetCardProcessRunner::new(
			fake_command("reset-card", temp.path(), None),
			binding(),
			Duration::from_secs(2),
		);
		let inventory = runner
			.clone()
			.read_inventory(&FixtureVault::matching(), capacity.reserve().unwrap())
			.unwrap();

		assert_eq!(inventory.available_count(), 1);
		assert_eq!(
			inventory
				.resolve_exact_credit_id(
					decodex_core::ResetCardDescriptor::new(
						decodex_core::ResetCardTimestamp::from_unix_seconds(1_700_000_000).unwrap(),
						decodex_core::ResetCardTimestamp::from_unix_seconds(1_700_003_600).unwrap(),
					)
					.unwrap(),
				)
				.unwrap()
				.as_str(),
			"fixture-reset-credit",
		);

		let readback = runner
			.consume_and_readback(
				&FixtureVault::matching(),
				capacity.reserve().unwrap(),
				ExactResetCreditId::new("fixture-reset-credit").unwrap(),
				ResetCardIdempotencyKey::new("fixture-reset-operation").unwrap(),
			)
			.unwrap();

		assert_eq!(readback.outcome, ResetCardConsumeOutcome::Reset);
		assert_eq!(readback.inventory.available_count(), 0);
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn reset_card_runner_retries_an_incomplete_positive_inventory() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let runner = ResetCardProcessRunner::new(
			fake_command("reset-card-partial-first", temp.path(), None),
			binding(),
			Duration::from_secs(2),
		);

		let inventory =
			runner.read_inventory(&FixtureVault::matching(), capacity.reserve().unwrap()).unwrap();

		assert!(inventory.details_complete());
		assert_eq!(inventory.reported_available_count(), Some(1));
		assert_eq!(inventory.available_count(), 1);
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn reset_card_runner_retries_an_inventory_with_missing_count_and_details() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let runner = ResetCardProcessRunner::new(
			fake_command("reset-card-missing-first", temp.path(), None),
			binding(),
			Duration::from_secs(2),
		);

		let inventory =
			runner.read_inventory(&FixtureVault::matching(), capacity.reserve().unwrap()).unwrap();

		assert!(inventory.details_complete());
		assert_eq!(inventory.reported_available_count(), Some(1));
		assert_eq!(inventory.available_count(), 1);
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn callback_probe_uses_one_initial_synthetic_login_then_re_attests_the_real_successor() {
		let temp = TempDir::new().unwrap();
		let timeout = Duration::from_secs(2);
		let capacity = TestCapacity::new(1);
		let command = fake_command("callback-probe", temp.path(), None);
		let callback = Arc::new(FixtureRefreshCallback::default());
		let process_binding =
			callback_binding(Arc::clone(&callback) as Arc<dyn AccountRefreshCallback>);
		let (build, generated, guard) = process::attest_executable(
			&command,
			&process_binding,
			timeout,
			Some(capacity.reserve().unwrap()),
		)
		.unwrap();
		let process = SupervisedProcess::spawn_bound(
			command,
			process_binding,
			guard.expect("the test supplied one runner permit"),
		)
		.unwrap();
		let mut child =
			AttestedProcessChild { process, build, generated, timeout, initialized: false };

		child.initialize_callback_probe(&CallbackProbeFixtureVault).unwrap();
		child.prove_refresh_callback().unwrap();

		assert_eq!(callback.calls.load(Ordering::Acquire), 1);
		let AttestedProcessChild { process, .. } = child;
		process.shutdown(Duration::from_secs(1)).unwrap();
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn reset_card_rpc_errors_distinguish_missing_methods_from_provider_failures() {
		assert_eq!(
			ResetCardProcessError::from_rpc(
				RpcError::MethodRejected(-32_601),
				ResetCardProcessMethod::InventoryRead,
			),
			ResetCardProcessError::MethodUnavailable(ResetCardProcessMethod::InventoryRead)
		);
		assert_eq!(
			ResetCardProcessError::from_rpc(
				RpcError::MethodRejected(-32_603),
				ResetCardProcessMethod::InventoryRead,
			),
			ResetCardProcessError::ProcessUnavailable
		);
	}

	#[cfg(test)]
	#[test]
	fn cross_adapter_fixture_runs_the_same_bound_mechanics() {
		let temp = TempDir::new().unwrap();
		let codex_home = temp.path().join("home/.codex");

		fs::create_dir_all(&codex_home).unwrap();

		let result = ReadOnlyProbe::fixture(
			AppServerCommand::fixture("normal", temp.path(), None),
			AccountBinding::fixture(binding().account_id().clone(), codex_home),
			Duration::from_secs(2),
		)
		.run_bound_for_test(&FixtureVault::matching(), &mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.account_id, *binding().account_id());
	}

	#[test]
	fn manual_vault_projection_attests_the_exact_account_and_process() {
		let temp = TempDir::new().unwrap();
		let vault = FixtureVault::matching();
		let before = process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire);
		let result = ReadOnlyProbe::new_for_test(
			fake_command("normal", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_bound_for_test(&vault, &mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.account_id, binding().account_id().clone());
		assert_eq!(result.process_id, vault.process_id.load(Ordering::Acquire));
		assert!(process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire) > before);
	}

	#[test]
	fn default_vault_keeps_bound_runner_unavailable() {
		let temp = TempDir::new().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			fake_command("normal", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_bound_for_test(&UnavailableCredentialVault, &mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::CredentialVault(CredentialVaultError::Unavailable));
	}

	#[test]
	fn manual_bound_fixture_preserves_shared_auth_pool_and_plugin_files() {
		let temp = TempDir::new().unwrap();
		let home = temp.path().join("home");
		let codex_home = home.join(".codex");
		let plugin_dir = codex_home.join("plugins");

		fs::create_dir_all(&plugin_dir).unwrap();

		let fixtures = [
			(codex_home.join("auth.json"), b"{}".as_slice()),
			(
				codex_home.join("account-pool.json"),
				br#"{"account_id":"10000000-0000-4000-8000-000000000001","state":"available"}"#
					.as_slice(),
			),
			(plugin_dir.join("state.json"), br#"{"enabled":true}"#.as_slice()),
		];

		for (path, contents) in &fixtures {
			fs::write(path, contents).unwrap();
		}

		let before = fixtures
			.iter()
			.map(|(path, _)| (path.clone(), fs::read(path).unwrap()))
			.collect::<Vec<_>>();
		let bound_account = AccountBinding::for_test(codex_home);
		let result = ReadOnlyProbe::new_for_test(
			fake_command("normal", temp.path(), None),
			bound_account.clone(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_bound_for_test(&FixtureVault::matching(), &mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.account_id, bound_account.account_id().clone());

		for (path, contents) in before {
			assert_eq!(fs::read(path).unwrap(), contents);
		}
	}

	#[test]
	fn requested_and_observed_account_mismatch_terminates_fail_closed() {
		let temp = TempDir::new().unwrap();
		let vault =
			FixtureVault { expected_email: "different@example.test", ..FixtureVault::matching() };
		let error = ReadOnlyProbe::new_for_test(
			fake_command("normal", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_bound_for_test(&vault, &mut CapabilityCache::default())
		.unwrap_err();
		let process_id = vault.process_id.load(Ordering::Acquire);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::AccountChanged));
		assert!(!process::process_group_exists(process_id).unwrap());
	}

	#[test]
	fn credential_projection_cannot_switch_under_one_live_child() {
		let temp = TempDir::new().unwrap();
		let vault = FixtureVault { double_project: true, ..FixtureVault::matching() };
		let error = ReadOnlyProbe::new_for_test(
			fake_command("normal", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_bound_for_test(&vault, &mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::CredentialVault(CredentialVaultError::ProjectionAlreadyUsed));

		let debug = format!("{error:?}");

		assert!(!debug.contains("synthetic-nonsecret-sentinel"));
		assert!(!debug.contains("synthetic-provider-sentinel"));
	}

	#[test]
	fn credential_projection_rejects_and_redacts_unexpected_response_payload() {
		for mode in ["login-extra", "login-wrong-type", "login-missing-type"] {
			let temp = TempDir::new().unwrap();
			let error = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run_bound_for_test(&FixtureVault::matching(), &mut CapabilityCache::default())
			.unwrap_err();

			assert_eq!(
				error,
				ProbeError::CredentialVault(CredentialVaultError::ProjectionRejected)
			);
			assert!(!format!("{error:?}").contains("synthetic-nonsecret-sentinel"));
		}
	}

	#[test]
	fn credential_projection_response_requires_the_exact_chatgpt_token_variant() {
		assert!(
			serde_json::from_slice::<CredentialProjectionResponse>(
				br#"{"type":"chatgptAuthTokens"}"#
			)
			.is_ok()
		);

		for json in [
			br#"{}"#.as_slice(),
			br#"{"type":"chatgpt"}"#.as_slice(),
			br#"{"type":"chatgptAuthTokens","extra":true}"#.as_slice(),
		] {
			assert!(serde_json::from_slice::<CredentialProjectionResponse>(json).is_err());
		}
	}

	#[test]
	fn callback_probe_token_is_a_nonsecret_business_jwt_bound_to_the_provider() {
		let provider_account_id = "provider-account-for-callback-proof";
		let token = callback_probe_access_token(provider_account_id).unwrap();
		let parts = token.split('.').collect::<Vec<_>>();

		assert_eq!(parts.len(), 3);
		let header: Value =
			serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
		let payload: Value =
			serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
		assert_eq!(header, serde_json::json!({"alg": "none", "typ": "JWT"}));
		assert_eq!(payload["email"], CALLBACK_PROBE_EMAIL);
		assert_eq!(
			payload["https://api.openai.com/auth"]["chatgpt_plan_type"],
			CALLBACK_PROBE_PLAN_TYPE
		);
		assert!(payload["https://api.openai.com/auth"].get("chatgpt_user_id").is_none());
		assert_eq!(
			payload["https://api.openai.com/auth"]["chatgpt_account_id"],
			provider_account_id
		);
		assert_eq!(URL_SAFE_NO_PAD.decode(parts[2]).unwrap(), CALLBACK_PROBE_SIGNATURE);
		assert_ne!(*token, *callback_probe_access_token("different-provider-account").unwrap());
	}

	#[test]
	fn queued_late_typed_failure_zeroizes_completed_fields_and_raw_blocks() {
		let temp = TempDir::new().unwrap();
		let before = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);

		protocol::reset_sensitive_string_drops();

		let error = ReadOnlyProbe::new_for_test(
			fake_command("late-typed-error", temp.path(), None),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run_bound_for_test(&FixtureVault::matching(), &mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::InvalidProtocol));
		assert_eq!(protocol::sensitive_string_drops(), 3);
		assert!(process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire) > before);
	}

	#[test]
	fn escaped_success_and_error_frames_fail_before_typed_decode() {
		for mode in ["escaped-success", "escaped-error"] {
			let temp = TempDir::new().unwrap();
			let before = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);
			let error = ReadOnlyProbe::new_for_test(
				fake_command(mode, temp.path(), None),
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(SupervisionError::InvalidProtocol));
			assert!(process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire) > before);
		}
	}

	#[test]
	fn unexercised_optional_methods_remain_not_probed() {
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
				&CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }
			);
			assert!(result.profile.contradictions().is_empty());
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
					&CapabilityState::Unsupported { reason: UnsupportedReason::SchemaMissing }
				);
			}
		}
	}

	#[test]
	fn generated_schema_failure_occurs_before_app_server_spawn() {
		let temp = TempDir::new().unwrap();
		let marker_path = temp.path().join("spawned");
		let result = ReadOnlyProbe::new_for_test(
			fake_command("schema-missing", temp.path(), Some(&marker_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run(&mut CapabilityCache::default());

		assert!(matches!(result, Err(ProbeError::SchemaMissing { .. })));
		assert!(!marker_path.exists());
	}

	#[test]
	fn retired_nested_refresh_schema_layout_fails_before_app_server_spawn() {
		let temp = TempDir::new().unwrap();
		let marker_path = temp.path().join("spawned");
		let result = ReadOnlyProbe::new_for_test(
			fake_command("nested-refresh-schema", temp.path(), Some(&marker_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run(&mut CapabilityCache::default());

		assert_eq!(
			result.unwrap_err(),
			ProbeError::SchemaMissing { markers: vec!["schema:document".into()] }
		);
		assert!(!marker_path.exists());
	}

	#[test]
	fn generated_schema_fingerprint_is_observed_without_a_release_pin() {
		let temp = TempDir::new().unwrap();
		let marker_path = temp.path().join("spawned");
		let result = ReadOnlyProbe::new(
			fake_command("mark-spawn", temp.path(), Some(&marker_path)),
			binding(),
			Duration::from_secs(5),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.profile.schema_fingerprint().len(), 64);
		assert!(marker_path.exists());
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
				Duration::from_secs(5),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert!(
				matches!(error, ProbeError::SchemaMissing { .. }),
				"{mode} returned an unexpected error: {error:?}"
			);
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
				Duration::from_secs(5),
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
			Duration::from_secs(5),
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
			Duration::from_secs(5),
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
			Duration::from_secs(5),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert_eq!(
			result.profile.state(Capability::NativeCollaboration),
			&CapabilityState::Unsupported { reason: UnsupportedReason::SchemaMissing }
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
		let (sender, receiver) = mpsc::sync_channel(PROTOCOL_QUEUE_CAPACITY);
		let exceeded = Arc::new(AtomicBool::new(false));
		let zeroized_before = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);

		process::pump_stdout(
			Cursor::new(frames),
			sender,
			Arc::clone(&exceeded),
			&AtomicBool::new(false),
			None,
		);

		drop(receiver);

		assert!(exceeded.load(Ordering::Acquire));
		assert!(
			process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire) - zeroized_before
				>= PROTOCOL_QUEUE_CAPACITY + 2
		);
	}

	#[test]
	fn escaped_inbound_strings_are_rejected_before_serde_scratch() {
		assert!(
			SupervisedProcess::validate_zero_scratch_json(
				br#"{"id":1,"result":{"email":"plain@example.test"}}"#
			)
			.is_ok()
		);

		for frame in [
			br#"{"id":1,"result":{"email":"secret\\path"}}"#.as_slice(),
			br#"{"id":1,"error":{"message":"secret\"quote"}}"#.as_slice(),
			br#"{"method":"secret\nline"}"#.as_slice(),
			br#"{"result":{"nested":{"value":"secret\u0041"}}}"#.as_slice(),
		] {
			assert_eq!(SupervisedProcess::validate_zero_scratch_json(frame), Err(()));
		}
	}

	#[test]
	fn stdout_pump_cancellation_joins_and_wipes_partial_trailing_bytes() {
		let mut child = Command::new("/usr/bin/python3")
			.args([
				"-c",
				"import subprocess,sys,time; sys.stdout.write('trailing-secret'); sys.stdout.flush(); subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(.2)'], start_new_session=True); time.sleep(.2)",
			])
			.stdout(Stdio::piped())
			.spawn()
			.unwrap();
		let stdout = child.stdout.take().unwrap();
		let (sender, receiver) = mpsc::sync_channel(1);
		let before = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);
		let buffered = Arc::new(AtomicBool::new(false));
		let mut pump = process::StdoutPump::start_with_buffer_barrier(
			stdout,
			sender,
			Arc::new(AtomicBool::new(false)),
			Arc::clone(&buffered),
		)
		.unwrap();
		let deadline = Instant::now() + Duration::from_secs(2);

		while !buffered.load(Ordering::Acquire) && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(5));
		}

		assert!(buffered.load(Ordering::Acquire), "pump did not own trailing bytes");
		assert!(pump.stop(Duration::from_millis(250)));

		drop(receiver);

		assert!(process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire) - before >= 2);

		let _ = child.kill();
		let _ = child.wait();

		thread::sleep(Duration::from_millis(250));
	}

	#[test]
	fn disconnected_partial_and_buffered_inbound_paths_zeroize_every_owner() {
		let before = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);
		let (sender, receiver) = mpsc::sync_channel(1);

		drop(receiver);

		process::pump_stdout(
			Cursor::new(b"disconnected-sentinel\n"),
			sender,
			Arc::new(AtomicBool::new(false)),
			&AtomicBool::new(false),
			None,
		);

		let after_disconnect = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);

		assert!(after_disconnect - before >= 2, "frame and read buffer must both be wiped");

		let (sender, receiver) = mpsc::sync_channel(1);

		process::pump_stdout(
			Cursor::new(b"first-sentinel\nsecond-sentinel\n"),
			sender,
			Arc::new(AtomicBool::new(false)),
			&AtomicBool::new(false),
			None,
		);

		drop(receiver);

		assert!(
			process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire) - after_disconnect >= 3,
			"queued, overflowed, and read-buffer owners must all be wiped"
		);
	}

	#[test]
	fn partial_eof_read_error_and_parse_error_zeroize_inbound_storage() {
		struct PartialThenError {
			returned: bool,
		}

		impl io::Read for PartialThenError {
			fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
				if self.returned {
					return Err(io::Error::other("synthetic read failure"));
				}

				self.returned = true;

				let bytes = b"partial-error-sentinel";

				buffer[..bytes.len()].copy_from_slice(bytes);

				Ok(bytes.len())
			}
		}

		let before = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);
		let (sender, receiver) = mpsc::sync_channel(1);

		process::pump_stdout(
			Cursor::new(b"partial-eof-sentinel"),
			sender,
			Arc::new(AtomicBool::new(false)),
			&AtomicBool::new(false),
			None,
		);

		let frame = receiver.recv().unwrap();
		let bytes = frame.into_contiguous();

		assert_eq!(&*bytes, b"partial-eof-sentinel");

		drop(bytes);
		drop(receiver);

		let after_eof = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);

		assert!(after_eof - before >= 2);

		let (sender, receiver) = mpsc::sync_channel(1);

		process::pump_stdout(
			PartialThenError { returned: false },
			sender,
			Arc::new(AtomicBool::new(false)),
			&AtomicBool::new(false),
			None,
		);

		drop(receiver);

		let after_error = process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire);

		assert!(after_error - after_eof >= 2);

		let mut invalid = process::InboundFrame::new();

		invalid
			.extend_from_slice(
				br#"{"id":1,"result":{"codexHome":"/tmp/\u0073ecret","platformFamily":"u\u006eix","platformOs":"test"},"error":null}
"#,
			)
			.unwrap();

		let invalid = invalid.into_contiguous();

		protocol::reset_sensitive_string_drops();

		assert!(serde_json::from_slice::<JsonRpcResponse<InitializeResponse>>(&invalid).is_err());
		assert_eq!(protocol::sensitive_string_drops(), 3);

		drop(invalid);

		assert!(process::ZEROIZED_INBOUND_BLOCKS.load(Ordering::Acquire) > after_error);
	}

	#[test]
	fn outbound_json_rpc_frames_are_bounded_before_write() {
		let before = process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire);
		let temp = TempDir::new().unwrap();
		let mut process =
			SupervisedProcess::spawn(fake_command("normal", temp.path(), None), binding()).unwrap();
		let oversized =
			serde_json::json!({"value": "x".repeat(process::MAX_APP_SERVER_FRAME_BYTES)});

		assert_eq!(
			process.write_json(&oversized),
			Err(ProbeError::Supervision(SupervisionError::ProtocolLimitExceeded))
		);
		assert!(process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire) > before);
	}

	#[test]
	fn outbound_json_blocks_wipe_after_growth_late_error_and_write_failure() {
		let value = "x".repeat(process::OUTBOUND_BLOCK_BYTES * 3);
		let before_growth = process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire);
		let frame = process::ZeroizingOutboundFrame::serialize(&serde_json::json!({
			"accessToken": &value,
		}))
		.unwrap();

		assert!(frame.blocks.len() >= 4);

		drop(frame);

		let after_growth = process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire);

		assert!(after_growth - before_growth >= 4);

		let error = process::ZeroizingOutboundFrame::serialize(&LateSerializationFailure(&value));

		assert!(matches!(error, Err(ProbeError::Supervision(SupervisionError::WriteFailed))));

		let after_error = process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire);

		assert!(after_error - after_growth >= 3);

		let frame = process::ZeroizingOutboundFrame::serialize(&serde_json::json!({
			"accessToken": &value,
		}))
		.unwrap();
		let blocks = frame.blocks.len();
		let before_write = process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire);

		assert_eq!(
			frame.write_to(&mut FailAfterOneWrite(false)),
			Err(ProbeError::Supervision(SupervisionError::WriteFailed))
		);

		drop(frame);

		assert!(process::ZEROIZED_OUTBOUND_BLOCKS.load(Ordering::Acquire) - before_write >= blocks);
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
				Duration::from_secs(5),
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
		let timeout = Duration::from_secs(5);
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
		let executable = temp.path().join("fake-codex");
		let command = replaceable_fake_command("normal", temp.path(), &executable);

		fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
		fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

		let error = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run(&mut CapabilityCache::default())
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ExecutableChanged));
	}

	#[test]
	fn executable_replacement_between_verification_and_exec_never_runs() {
		for trigger_spawn in [1, 3] {
			let temp = TempDir::new().unwrap();
			let source = temp.path().join("verified-python");
			let displaced = temp.path().join("verified-python.displaced");
			let attacker_marker = temp.path().join("unverified-image-ran");
			let source_for_action = source.clone();
			let marker_for_action = attacker_marker.clone();
			let displaced_for_action = displaced.clone();
			let spawn_count = Arc::new(AtomicU32::new(0));
			let command = replaceable_fake_command("normal", temp.path(), &source)
				.with_before_spawn_for_test(
					trigger_spawn,
					Arc::clone(&spawn_count),
					Arc::new(move || {
						fs::rename(&source_for_action, &displaced_for_action).unwrap();
						fs::write(
							&source_for_action,
							format!(
								"#!/bin/sh\nprintf ran > '{}'\nexit 0\n",
								marker_for_action.display()
							),
						)
						.unwrap();
						fs::set_permissions(&source_for_action, fs::Permissions::from_mode(0o755))
							.unwrap();
					}),
				);
			let error = ReadOnlyProbe::new_for_test(
				command,
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run(&mut CapabilityCache::default())
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(SupervisionError::ExecutableChanged));
			assert_eq!(spawn_count.load(Ordering::Acquire), trigger_spawn);
			assert!(!attacker_marker.exists());
		}
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn verified_snapshot_cannot_be_replaced_between_check_and_exec() {
		let temp = TempDir::new().unwrap();
		let command = fake_command("normal", temp.path(), None);
		let snapshot = command.executable.path.clone();
		let replacement = temp.path().join("replacement-image");

		fs::copy(&command.program, &replacement).unwrap();

		let snapshot_for_action = snapshot.clone();
		let replacement_for_action = replacement.clone();
		let command = command.with_before_spawn_for_test(
			3,
			Arc::new(AtomicU32::new(0)),
			Arc::new(move || {
				assert!(fs::write(&snapshot_for_action, b"unverified").is_err());
				assert!(fs::rename(&replacement_for_action, &snapshot_for_action).is_err());
			}),
		);
		let result = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert!(result.process_id > 0);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn canonical_image_replacement_after_verification_never_reaches_user_code() {
		let temp = TempDir::new().unwrap();
		let source = temp.path().join("verified-image");
		let marker = temp.path().join("replacement-ran");
		let source_for_action = source.clone();
		let spawn_count = Arc::new(AtomicU32::new(0));

		fs::copy("/bin/cat", &source).unwrap();
		fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
		let source_metadata = fs::metadata(&source).unwrap();
		let source_device = source_metadata.dev();
		let source_inode = source_metadata.ino();

		let command = AppServerCommand::new_for_test(
			&source,
			[
				OsString::from("-c"),
				OsString::from("printf ran > \"$1\""),
				OsString::from("decodex-attested-spawn"),
				marker.as_os_str().to_owned(),
			],
			std::iter::empty::<OsString>(),
			std::iter::empty::<OsString>(),
			temp.path(),
		)
		.with_attested_spawn_for_test()
		.with_after_verification_for_test(
			1,
			Arc::clone(&spawn_count),
			Arc::new(move || {
				fs::copy("/bin/sh", &source_for_action).unwrap();
				fs::set_permissions(&source_for_action, fs::Permissions::from_mode(0o755)).unwrap();
				let replacement_metadata = fs::metadata(&source_for_action).unwrap();

				assert_eq!(replacement_metadata.dev(), source_device);
				assert_eq!(replacement_metadata.ino(), source_inode);
			}),
		);
		let error = SupervisedProcess::spawn(command, binding()).unwrap_err();

		assert_eq!(error, SupervisionError::ExecutableChanged);
		assert_eq!(spawn_count.load(Ordering::Acquire), 1);
		assert!(!marker.exists());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn profile_launch_uses_suspended_dynamic_identity_after_metadata_checks() {
		let temp = TempDir::new().unwrap();
		let source = temp.path().join("verified-profile-image");
		let marker = temp.path().join("replacement-ran");
		let source_for_action = source.clone();
		let spawn_count = Arc::new(AtomicU32::new(0));

		fs::copy("/bin/cat", &source).unwrap();
		fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();

		let command = AppServerCommand::new_for_test(
			&source,
			[
				OsString::from("-c"),
				OsString::from("printf ran > \"$1\""),
				OsString::from("decodex-attested-profile-spawn"),
				marker.as_os_str().to_owned(),
			],
			std::iter::empty::<OsString>(),
			std::iter::empty::<OsString>(),
			temp.path(),
		)
		.with_attested_spawn_for_test()
		.with_after_verification_for_test(
			1,
			Arc::clone(&spawn_count),
			Arc::new(move || {
				fs::copy("/bin/sh", &source_for_action).unwrap();
				fs::set_permissions(&source_for_action, fs::Permissions::from_mode(0o755)).unwrap();
			}),
		);
		let error = SupervisedProcess::spawn_attested(
			command,
			binding(),
			TestCapacity::new(1).reserve().unwrap(),
			process::ExactBuildLaunchCapability::PrivateStdioDisabledEphemeralStartupV1,
		)
		.unwrap_err();

		assert_eq!(error, SupervisionError::SpawnFailed);
		assert_eq!(spawn_count.load(Ordering::Acquire), 1);
		assert!(!marker.exists());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn profile_launch_rejects_path_replacement_at_the_second_metadata_check() {
		let temp = TempDir::new().unwrap();
		let source = temp.path().join("verified-profile-image");
		let displaced = temp.path().join("verified-profile-image.displaced");
		let marker = temp.path().join("replacement-ran");
		let source_for_action = source.clone();
		let displaced_for_action = displaced.clone();
		let spawn_count = Arc::new(AtomicU32::new(0));

		fs::copy("/bin/cat", &source).unwrap();
		fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();

		let command = AppServerCommand::new_for_test(
			&source,
			[
				OsString::from("-c"),
				OsString::from("printf ran > \"$1\""),
				OsString::from("decodex-attested-profile-spawn"),
				marker.as_os_str().to_owned(),
			],
			std::iter::empty::<OsString>(),
			std::iter::empty::<OsString>(),
			temp.path(),
		)
		.with_attested_spawn_for_test()
		.with_before_spawn_for_test(
			1,
			Arc::clone(&spawn_count),
			Arc::new(move || {
				fs::rename(&source_for_action, &displaced_for_action).unwrap();
				fs::copy("/bin/sh", &source_for_action).unwrap();
				fs::set_permissions(&source_for_action, fs::Permissions::from_mode(0o755)).unwrap();
			}),
		);
		let error = SupervisedProcess::spawn_attested(
			command,
			binding(),
			TestCapacity::new(1).reserve().unwrap(),
			process::ExactBuildLaunchCapability::PrivateStdioDisabledEphemeralStartupV1,
		)
		.unwrap_err();

		assert_eq!(error, SupervisionError::ExecutableChanged);
		assert_eq!(spawn_count.load(Ordering::Acquire), 1);
		assert!(!marker.exists());
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn sealed_snapshot_is_the_image_executed_after_final_verification() {
		let temp = TempDir::new().unwrap();
		let source = temp.path().join("verified-python");
		let displaced = temp.path().join("verified-python.displaced");
		let source_for_action = source.clone();
		let displaced_for_action = displaced.clone();
		let command = replaceable_fake_command("normal", temp.path(), &source);
		let snapshot = command.executable.file.try_clone().unwrap();
		let command = command.with_after_verification_for_test(
			3,
			Arc::new(AtomicU32::new(0)),
			Arc::new(move || {
				let error =
					std::os::unix::fs::FileExt::write_at(&snapshot, b"unverified", 0).unwrap_err();

				assert_eq!(error.raw_os_error(), Some(libc::EPERM));

				fs::rename(&source_for_action, &displaced_for_action).unwrap();
				fs::copy("/bin/false", &source_for_action).unwrap();
				fs::set_permissions(&source_for_action, fs::Permissions::from_mode(0o755)).unwrap();
			}),
		);
		let result = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert!(result.process_id > 0);
	}

	#[cfg(unix)]
	#[test]
	fn preflight_waits_for_post_reap_descendant_quiescence_before_app_server_spawn() {
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
	fn failed_preflight_waits_for_post_reap_descendant_quiescence() {
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

	#[cfg(unix)]
	#[test]
	fn first_preflight_uncertain_cleanup_retains_the_only_capacity_slot() {
		assert_uncertain_preflight_retains_capacity("preflight-uncertain-version", 1);
	}

	#[cfg(unix)]
	#[test]
	fn second_preflight_uncertain_cleanup_retains_the_only_capacity_slot() {
		assert_uncertain_preflight_retains_capacity("preflight-uncertain-schema", 2);
	}

	#[cfg(unix)]
	#[test]
	fn repeated_cleanup_worker_start_failure_cannot_admit_a_process_or_permit() {
		for _ in 0..3 {
			assert!(matches!(
				ProcessQuarantine::try_new_with_worker_start_failure(),
				Err(SupervisionError::CleanupUnavailable)
			));
		}
	}

	#[test]
	fn coordinator_start_failure_joins_the_already_started_worker() {
		let lifecycle = Arc::new(process::QuarantineLifecycleProbe::default());

		assert!(matches!(
			ProcessQuarantine::try_new_with_coordinator_start_failure(Arc::clone(&lifecycle)),
			Err(SupervisionError::CleanupUnavailable)
		));
		assert!(lifecycle.started.load(Ordering::Acquire));
		assert!(lifecycle.exited.load(Ordering::Acquire));
		assert!(lifecycle.joined.load(Ordering::Acquire));
	}

	#[test]
	fn repeated_quarantine_construction_and_drop_joins_each_worker() {
		for _ in 0..8 {
			let quarantine = ProcessQuarantine::new();
			let lifecycle = quarantine.lifecycle_probe();

			wait_for_flag(&lifecycle.started, Duration::from_secs(1));
			drop(quarantine);
			wait_for_flag(&lifecycle.exited, Duration::from_secs(1));
			wait_for_flag(&lifecycle.joined, Duration::from_secs(1));
		}
	}

	#[cfg(unix)]
	#[test]
	fn dropping_capacity_waits_for_queued_cleanup_before_worker_exit() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let quarantine = Arc::clone(&capacity.inner.inner.quarantine);
		let lifecycle = quarantine.lifecycle_probe();
		let command = fake_command("preflight-uncertain-version", temp.path(), None)
			.with_preflight_cleanup_control_for_test(
				1,
				Arc::new(AtomicU32::new(0)),
				Arc::new(AtomicU32::new(0)),
				Duration::from_millis(150),
				Arc::clone(&quarantine),
			);
		let error = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_mechanical_with_lifetime_guard(
			&UnavailableCredentialVault,
			&mut CapabilityCache::default(),
			capacity.reserve().unwrap(),
		)
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		assert_eq!(capacity.active(), 1);

		drop(quarantine);
		drop(capacity);
		wait_for_flag(&lifecycle.exited, Duration::from_secs(3));
		wait_for_flag(&lifecycle.joined, Duration::from_secs(3));
	}

	#[cfg(unix)]
	#[test]
	fn worker_panic_after_pop_reinstalls_job_before_recovery() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let quarantine = ProcessQuarantine::new();

		quarantine.panic_after_next_worker_pop();

		let command = fake_command("preflight-uncertain-version", temp.path(), None)
			.with_preflight_cleanup_control_for_test(
				1,
				Arc::new(AtomicU32::new(0)),
				Arc::new(AtomicU32::new(0)),
				Duration::from_millis(250),
				quarantine,
			);
		let error = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_mechanical_with_lifetime_guard(
			&UnavailableCredentialVault,
			&mut CapabilityCache::default(),
			capacity.reserve().unwrap(),
		)
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		assert_eq!(capacity.active(), 1);

		wait_for_capacity(&capacity, 0, Duration::from_secs(3));
	}

	#[cfg(unix)]
	#[test]
	fn poisoned_queue_and_waiting_worker_recover_on_real_submissions() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let quarantine = ProcessQuarantine::new();
		let poisoned = Arc::clone(&quarantine);

		assert!(
			thread::spawn(move || {
				let _state = poisoned.state.wake.lock().unwrap();

				panic!("inject quarantine queue poison");
			})
			.join()
			.is_err()
		);

		for _ in 0..2 {
			let command = fake_command("preflight-uncertain-version", temp.path(), None)
				.with_preflight_cleanup_control_for_test(
					1,
					Arc::new(AtomicU32::new(0)),
					Arc::new(AtomicU32::new(0)),
					Duration::ZERO,
					Arc::clone(&quarantine),
				);
			let error = ReadOnlyProbe::new_for_test(
				command,
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run_mechanical_with_lifetime_guard(
				&UnavailableCredentialVault,
				&mut CapabilityCache::default(),
				capacity.reserve().unwrap(),
			)
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));

			wait_for_capacity(&capacity, 0, Duration::from_secs(3));

			thread::sleep(Duration::from_millis(50));
		}
	}

	#[cfg(unix)]
	#[test]
	fn uncertain_first_cleanup_does_not_starve_later_independent_group() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(2);
		let quarantine = ProcessQuarantine::new();
		let first_group = Arc::new(AtomicU32::new(0));
		let second_group = Arc::new(AtomicU32::new(0));

		for (group, delay) in [
			(Arc::clone(&first_group), Duration::from_secs(1)),
			(Arc::clone(&second_group), Duration::ZERO),
		] {
			let command = fake_command("preflight-uncertain-version", temp.path(), None)
				.with_preflight_cleanup_control_for_test(
					1,
					Arc::new(AtomicU32::new(0)),
					group,
					delay,
					Arc::clone(&quarantine),
				);
			let error = ReadOnlyProbe::new_for_test(
				command,
				binding(),
				SchemaMarker::accepted(),
				Duration::from_secs(2),
			)
			.run_mechanical_with_lifetime_guard(
				&UnavailableCredentialVault,
				&mut CapabilityCache::default(),
				capacity.reserve().unwrap(),
			)
			.unwrap_err();

			assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		}

		wait_for_capacity(&capacity, 1, Duration::from_millis(750));

		assert!(process::process_group_exists(first_group.load(Ordering::Acquire)).unwrap());
		assert!(
			!process::process_group_exists(second_group.load(Ordering::Acquire)).unwrap_or(true)
		);

		wait_for_capacity(&capacity, 0, Duration::from_secs(3));
	}

	#[test]
	fn preflight_spawn_refusal_before_child_creation_returns_capacity() {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let mut command = fake_command("normal", temp.path(), None);

		command.working_directory = temp.path().join("missing-working-directory");

		let permit = capacity.reserve().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run_mechanical_with_lifetime_guard(
			&UnavailableCredentialVault,
			&mut CapabilityCache::default(),
			permit,
		)
		.unwrap_err();

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	#[ignore = "requires installed Codex; sends only schema/version and read-only RPCs"]
	fn live_read_only_probe_negotiates_without_dispatch() {
		let command = AppServerCommand::new(env::current_dir().unwrap()).unwrap();
		let result = ReadOnlyProbe::new(
			command,
			AccountBinding::shared_home(
				AccountId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			)
			.unwrap(),
			Duration::from_secs(10),
		)
		// The debug test profile repeatedly hashes the large canonical executable and its
		// snapshot. Extend only attestation; keep each live protocol operation at ten seconds.
		.with_attestation_timeout_for_test(Duration::from_secs(60))
		.run(&mut CapabilityCache::default())
		.unwrap();

		assert_eq!(result.profile.state(Capability::Initialize), &CapabilityState::Supported);
		assert_eq!(result.profile.state(Capability::ThreadList), &CapabilityState::Supported);
	}

	#[cfg(unix)]
	#[test]
	fn shutdown_retains_capacity_until_post_reap_descendant_quiescence() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("descendant.pid");
		let capacity = TestCapacity::new(1);
		let process = SupervisedProcess::spawn_bound(
			fake_command("orphan-exit", temp.path(), Some(&pid_path)),
			binding(),
			capacity.reserve().unwrap(),
		)
		.unwrap();
		let deadline = Instant::now() + Duration::from_secs(2);

		while !pid_path.exists() && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		let descendant = read_pid(&pid_path);

		assert!(process_exists(descendant));
		assert_eq!(capacity.active(), 1);

		process.shutdown(Duration::from_secs(1)).unwrap();

		assert!(!process_exists(descendant), "descendant did not reach natural quiescence");
		assert_eq!(capacity.active(), 0);
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
	fn probe_error_retains_capacity_until_post_reap_descendant_quiescence() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("error-descendant.pid");
		let capacity = TestCapacity::new(1);
		let error = ReadOnlyProbe::new_for_test(
			fake_command("orphan-error", temp.path(), Some(&pid_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run_inner(None, Some(capacity.reserve().unwrap()), &mut CapabilityCache::default())
		.unwrap_err();
		let descendant = read_pid(&pid_path);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::InvalidProtocol));
		assert!(process_exists(descendant));
		assert_eq!(capacity.active(), 1);
		assert_eq!(capacity.reserve().unwrap_err(), ());

		wait_until_process_is_dead(descendant);
		wait_for_capacity(&capacity, 0, Duration::from_secs(3));
	}

	#[cfg(unix)]
	#[test]
	fn probe_timeout_retains_capacity_until_post_reap_descendant_quiescence() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("timeout-descendant.pid");
		let capacity = TestCapacity::new(1);
		let error = ReadOnlyProbe::new_for_test(
			fake_command("orphan-timeout", temp.path(), Some(&pid_path)),
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(5),
		)
		.run_inner(None, Some(capacity.reserve().unwrap()), &mut CapabilityCache::default())
		.unwrap_err();
		let descendant = read_pid(&pid_path);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::ResponseTimeout));
		assert!(process_exists(descendant));
		assert_eq!(capacity.active(), 1);
		assert_eq!(capacity.reserve().unwrap_err(), ());

		wait_until_process_is_dead(descendant);
		wait_for_capacity(&capacity, 0, Duration::from_secs(3));
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
	fn bound_capacity_follows_the_live_group_into_reaper_cleanup() {
		let temp = TempDir::new().unwrap();
		let pid_path = temp.path().join("capacity-reaper-descendant.pid");
		let capacity = TestCapacity::new(1);
		let permit = capacity.reserve().unwrap();
		let process = SupervisedProcess::spawn_bound(
			fake_command("orphan-stubborn", temp.path(), Some(&pid_path)),
			binding(),
			permit,
		)
		.unwrap();
		let descendant = read_pid(&pid_path);

		assert!(process_exists(descendant));
		assert_eq!(capacity.active(), 1);
		assert_eq!(process.shutdown(Duration::ZERO), Err(SupervisionError::ShutdownFailed));

		wait_until_process_is_dead(descendant);

		let deadline = Instant::now() + Duration::from_secs(2);

		while capacity.active() != 0 && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		assert_eq!(capacity.active(), 0);
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

	fn wait_for_flag(flag: &AtomicBool, timeout: Duration) {
		let deadline = Instant::now() + timeout;

		while !flag.load(Ordering::Acquire) && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		assert!(flag.load(Ordering::Acquire), "quarantine lifecycle transition timed out");
	}

	#[cfg(unix)]
	fn assert_uncertain_preflight_retains_capacity(mode: &str, trigger_spawn: u32) {
		let temp = TempDir::new().unwrap();
		let capacity = TestCapacity::new(1);
		let spawn_count = Arc::new(AtomicU32::new(0));
		let process_group = Arc::new(AtomicU32::new(0));
		let command = fake_command(mode, temp.path(), None).with_uncertain_preflight_for_test(
			trigger_spawn,
			Arc::clone(&spawn_count),
			Arc::clone(&process_group),
			Duration::from_millis(500),
		);
		let permit = capacity.reserve().unwrap();
		let error = ReadOnlyProbe::new_for_test(
			command,
			binding(),
			SchemaMarker::accepted(),
			Duration::from_secs(2),
		)
		.run_mechanical_with_lifetime_guard(
			&UnavailableCredentialVault,
			&mut CapabilityCache::default(),
			permit,
		)
		.unwrap_err();
		let group = process_group.load(Ordering::Acquire);

		assert_eq!(error, ProbeError::Supervision(SupervisionError::PreflightFailed));
		assert_eq!(spawn_count.load(Ordering::Acquire), trigger_spawn);
		assert_ne!(group, 0);
		assert!(process::process_group_exists(group).unwrap());
		assert_eq!(capacity.active(), 1);

		for _ in 0..3 {
			assert_eq!(capacity.reserve().unwrap_err(), ());
		}

		let deadline = Instant::now() + Duration::from_secs(3);

		while (process::process_group_exists(group).unwrap_or(true) || capacity.active() != 0)
			&& Instant::now() < deadline
		{
			thread::sleep(Duration::from_millis(10));
		}

		assert!(!process::process_group_exists(group).unwrap_or(true));
		assert_eq!(capacity.active(), 0);
	}

	#[cfg(unix)]
	fn wait_for_capacity(capacity: &TestCapacity, expected: u16, timeout: Duration) {
		let deadline = Instant::now() + timeout;

		while capacity.active() != expected && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(10));
		}

		assert_eq!(capacity.active(), expected);
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

	fn exact_filter(archived: ThreadArchivedFilter) -> ExactThreadListFilter {
		ExactThreadListFilter {
			search_term: DecodexThreadSearchTerm::new("Decodex XY-1317 exact reconciliation")
				.unwrap(),
			archived,
		}
	}

	fn exact_thread_id() -> ExactThreadId {
		ExactThreadId::new("thread:XY-1317/non-uuid_Case-Sensitive._~:@+$,;=[]{}()!%&'*? #")
			.unwrap()
	}

	fn initialized_bound_process(mode: &str) -> (TempDir, SupervisedProcess) {
		let temp = TempDir::new().unwrap();
		let timeout = Duration::from_secs(2);
		let mut process =
			SupervisedProcess::spawn(fake_command(mode, temp.path(), None), binding()).unwrap();

		initialize_test_process(&mut process, timeout);

		let expected = {
			let vault = FixtureVault::matching();
			let account_id = process.binding.account_id().clone();
			let mut projection =
				CredentialProjection { process: &mut process, timeout, used: false };

			vault.project(&account_id, &mut projection).unwrap()
		};

		process.expected_account_identity = Some(expected);

		process.read_account_identity(timeout).unwrap();

		(temp, process)
	}

	#[test]
	fn exact_non_uuid_identity_round_trips_through_list_read_archive_and_readback() {
		let (_temp, mut process) = initialized_bound_process("exact");
		let timeout = Duration::from_secs(2);
		let exact = exact_thread_id();
		let listed = process
			.list_exact_threads(&exact_filter(ThreadArchivedFilter::Current), timeout)
			.unwrap();

		assert_eq!(listed.threads().len(), 1);
		assert_eq!(listed.threads()[0].id, exact);
		assert_eq!(listed.threads()[0].created_at.unix_seconds(), 1_784_073_600);
		assert_eq!(
			listed.threads()[0].title.as_ref().unwrap().as_str(),
			"Decodex XY-1317 exact reconciliation"
		);
		assert_eq!(listed.threads()[0].cwd.as_str(), "/tmp/xy-1317-repository");
		assert_eq!(
			listed.threads()[0].provenance.as_ref().unwrap().as_str(),
			"decodex.xy1317.fixture"
		);
		assert!(!listed.threads()[0].archived);

		let read = process.read_exact_thread(&exact, timeout).unwrap();

		assert_eq!(read.facts.id, exact);
		assert_eq!(read.history, decodex_codex::LossyThreadHistory::IncludeTurnsReadback);
		assert_eq!(
			process.reconcile_archive(&exact, timeout),
			ArchiveReconciliationOutcome::Archived
		);
		assert_eq!(process.read_exact_thread(&exact, timeout).unwrap().facts.id, exact);
		assert_eq!(
			process
				.list_exact_threads(&exact_filter(ThreadArchivedFilter::Archived), timeout)
				.unwrap()
				.threads()[0]
				.id,
			exact
		);
		assert_eq!(
			process.reconcile_archive(&exact, timeout),
			ArchiveReconciliationOutcome::AlreadyArchived
		);
	}

	#[test]
	fn private_reconciliation_owner_runs_under_one_concrete_account_capacity_guard() {
		let temp = TempDir::new().unwrap();
		let binding = binding();
		let account_id = binding.account_id().clone();
		let capacity = TestCapacity::new(1);
		let result = ExactThreadReconciler::fixture(
			fake_command("exact", temp.path(), None),
			binding,
			Duration::from_secs(2),
		)
		.run_mechanical_with_lifetime_guard(
			&FixtureVault::matching(),
			&mut CapabilityCache::default(),
			capacity.inner.reserve(account_id, 1).unwrap(),
			ExactThreadReconciliation::List(exact_filter(ThreadArchivedFilter::Current)),
		)
		.unwrap();
		let ExactThreadReconciliationResult::List(list) = result else {
			panic!("private list operation returned a different closed result variant");
		};

		assert_eq!(list.threads()[0].id, exact_thread_id());
		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn abandoned_request_correlation_state_is_hard_bounded() {
		let (_temp, mut process) = initialized_bound_process("exact");

		for request_id in 1..=PROTOCOL_QUEUE_CAPACITY as u64 {
			process.abandon_request(request_id).unwrap();
		}

		assert_eq!(
			process.abandon_request(PROTOCOL_QUEUE_CAPACITY as u64 + 1),
			Err(super::RpcError::Supervision(SupervisionError::ProtocolLimitExceeded))
		);
	}

	#[test]
	fn dropped_archive_response_is_resolved_only_by_same_process_exact_readback() {
		let (_temp, mut process) = initialized_bound_process("exact-drop-after-apply");
		let exact = exact_thread_id();

		assert_eq!(
			process.reconcile_archive(&exact, Duration::from_millis(100)),
			ArchiveReconciliationOutcome::Archived
		);
	}

	#[test]
	fn contradictory_archive_readback_never_reports_success() {
		let (_temp, mut process) = initialized_bound_process("exact-contradictory-readback");
		let exact = exact_thread_id();

		assert_eq!(
			process.reconcile_archive(&exact, Duration::from_secs(2)),
			ArchiveReconciliationOutcome::Unverified(ArchiveUnverifiedReason::ReadbackFailed)
		);
	}

	#[test]
	fn missing_mismatched_and_still_ambiguous_archive_readback_fail_closed() {
		let exact = exact_thread_id();

		for (mode, reason) in [
			("exact-missing-post-archive-read", ArchiveUnverifiedReason::ReadbackFailed),
			("exact-mismatched-post-archive-read", ArchiveUnverifiedReason::ReadbackFailed),
			("exact-ambiguous-unapplied", ArchiveUnverifiedReason::AmbiguousMutation),
		] {
			let (_temp, mut process) = initialized_bound_process(mode);

			assert_eq!(
				process.reconcile_archive(&exact, Duration::from_millis(100)),
				ArchiveReconciliationOutcome::Unverified(reason),
				"mode {mode} did not fail closed"
			);
		}
	}

	#[test]
	fn unsupported_archive_is_a_closed_unverified_outcome() {
		let (_temp, mut process) = initialized_bound_process("exact-unsupported-archive");
		let exact = exact_thread_id();

		assert_eq!(
			process.reconcile_archive(&exact, Duration::from_secs(2)),
			ArchiveReconciliationOutcome::Unverified(ArchiveUnverifiedReason::MethodUnsupported)
		);
	}

	#[test]
	fn exact_list_rejects_malformed_wrong_correlation_and_missing_result() {
		for mode in ["exact-malformed-list", "exact-wrong-correlation", "exact-missing-result"] {
			let (_temp, mut process) = initialized_bound_process(mode);

			assert!(
				process
					.list_exact_threads(
						&exact_filter(ThreadArchivedFilter::Current),
						Duration::from_secs(2),
					)
					.is_err(),
				"mode {mode} unexpectedly succeeded"
			);
		}
	}

	#[test]
	fn exact_read_rejects_malformed_oversized_and_mismatched_results() {
		let exact = exact_thread_id();

		for mode in ["exact-malformed-read", "exact-oversized-read", "exact-mismatched-id"] {
			let (_temp, mut process) = initialized_bound_process(mode);

			assert!(
				process.read_exact_thread(&exact, Duration::from_millis(500)).is_err(),
				"mode {mode} unexpectedly succeeded"
			);
		}
	}

	#[test]
	fn exact_reconciliation_stops_on_account_binding_contradiction() {
		let (_temp, mut process) = initialized_bound_process("account-switch");

		assert_eq!(
			process.list_exact_threads(
				&exact_filter(ThreadArchivedFilter::Current),
				Duration::from_secs(2),
			),
			Err(super::ExactReconciliationError::AccountBindingChanged)
		);
	}
}
