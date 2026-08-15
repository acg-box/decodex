//! Ordinary Conversation use-case adapter for one short interactive task.
//!
//! Durable authority stays with Conversation, Routing Decision, Continuation Plan,
//! RuntimeSession, ProcessGeneration, and ProviderAttempt. This module retains only bounded
//! daemon-local process and active-turn handles.

use std::{
	collections::BTreeMap,
	ffi::{CStr, CString, OsStr},
	fmt::{Debug, Formatter},
	fs::{File, Metadata},
	mem::MaybeUninit,
	os::{
		fd::{AsRawFd as _, FromRawFd as _},
		unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
	},
	path::{Component, Path, PathBuf},
	ptr,
	sync::{Arc, Mutex, PoisonError, mpsc},
	time::{Duration, Instant},
};

use decodex_codex::{
	ArchiveReconciliationOutcome, ExactThreadId, MAX_QUICK_TASK_INPUT_BYTES,
	QuickTaskThreadResumeRequest,
	QuickTaskThreadStartRequest, QuickTaskTurnInput, QuickTaskTurnInterruptRequest,
	QuickTaskTurnStartRequest, QuickTaskTurnStatus, TurnStatus,
};
use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperationId, ContextPack, ContextPackInput,
	ContextPackPolicy, ContextPackSource, ContextSourceKind, ContinuationRejection, ConversationId,
	ExecutionConsumer, HistoryItemId, HistoryItemKind, HistoryMediaType, HistoryMetadata,
	ItemStatus, MAX_CONTEXT_PACK_BYTES, MAX_CONTEXT_RECENT_ITEMS, MIN_CONTEXT_PACK_BYTES,
	PinnedContextSource, PossibleSideEffects, ProcessExecutionAuthorization,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProviderAttemptConsumer,
	ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState, ProviderDuplicateRisk,
	ProviderEvidenceId, ProviderEvidenceSource, ProviderPositiveEvidence, ProviderRequestId,
	ProviderRequestKey, ProviderRequestKeys, ProviderTerminalOutcome, RuntimeSessionId,
	RuntimeSessionState, TurnId, TurnRole, compile_context_pack,
};
use decodex_database::{
	AdmitInitialQuickTaskTurn, ArchiveLocalQuickTaskConversation,
	ArchiveLocalQuickTaskConversationOutcome, ArchiveQuickTaskConversation,
	ArchiveQuickTaskConversationOutcome, AuthorizeProviderDispatchOutcome,
	BindRuntimeSessionThreadOutcome, CommandIdentity, CreateQuickTaskConversation,
	FenceRuntimeSessionThreadStart,
	FenceRuntimeSessionThreadStartOutcome, FreshQuickTaskProcessGeneration,
	InitialQuickTaskTurnAdmissionOutcome, OrdinaryRuntimeSessionResumeReadback,
	PrepareQuickTaskProcessGeneration, PrepareQuickTaskProcessGenerationOutcome,
	ProviderAttemptMutationOutcome, QuickTaskTerminalizationOutcome,
	QuickTaskThreadEstablishmentReadback,
	ReconcileQuickTaskThreadEstablishment, ReconcileStrandedQuickTaskTurn,
	ReconcileStrandedQuickTaskTurnOutcome, RecordHistoryItem, RoleProfileRole, SqliteStore, StoreError,
	TerminalizeQuickTaskTurn, TurnReservationOutcome,
};
use decodex_protocol::{HistoryText, MAX_HISTORY_INLINE_BYTES, QuickTaskUnavailableReason};
use sha2::{Digest as _, Sha256};
use tokio::{
	sync::{Mutex as AsyncMutex, mpsc as tokio_mpsc},
	task::{self, JoinSet},
};

use crate::{
	ProcessGenerationControl, ProviderAttemptControl, ProviderAttemptReconciliation,
	account_launch::{CapacityExhausted, RunnerCapacity},
	account_service::{AccountLifecycleError, AccountProcessCredential, AccountService},
	process_supervisor::{FencedProcess, ProcessGenerationTermination},
	provider_attempt_service::{
		FreshRuntimeSessionResume, ProviderAttemptRuntimeAuthority, RuntimeSessionResumeRequest,
		SuccessfulRuntimeSessionResume,
	},
	routing_orchestration::{
		ContinuationExecutionCommand, DefinitePostProcessRefusal, ExecutionCommand,
		ExecutionCoordinator, ExecutionFailureKind, PersistedDecisionProvenance,
		PostProcessCommand, PostProcessOutcome, PreProcessOutcome,
	},
};

use crate::account_launch::process::{
	AccountBinding, AccountIdentity, AccountRefreshCallback as ProcessAccountRefreshCallback,
	AttestedAppServerLaunch, AttestedAppServerProfile, AttestedProcessChild,
	ChatgptRefreshProjection, CredentialProjection, CredentialVault, CredentialVaultError,
	EstablishedOrdinaryThread, PreparedThreadStart, PreparedTurnStart, QuickTaskPreSpawnCheck,
	QuickTaskProcessError, QuickTaskProcessEvent, ResumedOrdinaryThread, StartedOrdinaryTurn,
	spawn_admitted_quick_task_process,
};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const TURN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const EVENT_POLL: Duration = Duration::from_millis(100);
const EVENT_QUEUE_CAPACITY: usize = 64;
const PROCESS_COMMAND_CAPACITY: usize = 2;
const MAX_LOCAL_TASKS: usize = 32;
const MAX_SELECTED_CWD_COMPONENTS: usize = 128;
const INITIAL_PASSWD_BUFFER_BYTES: usize = 16 * 1_024;
const MAX_PASSWD_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
struct DirectoryIdentity {
	device: u64,
	inode: u64,
	uid: u32,
	mode: u32,
}

impl DirectoryIdentity {
	fn from_metadata(metadata: &Metadata) -> Self {
		Self {
			device: metadata.dev(),
			inode: metadata.ino(),
			uid: metadata.uid(),
			mode: metadata.mode(),
		}
	}
}

/// Descriptor-retained proof for the one working directory selected under host policy.
struct SelectedWorkingDirectory {
	path: String,
	components: Vec<(File, DirectoryIdentity)>,
}

impl SelectedWorkingDirectory {
	fn acquire(path: &str) -> Result<Self, ()> {
		let path_value = Path::new(path);
		let effective_uid = unsafe { libc::geteuid() };
		let components = open_selected_path(path_value, effective_uid)?;
		let home = open_selected_path(&effective_user_home(effective_uid)?, effective_uid)?;
		if components.len() < home.len()
			|| components.iter().zip(&home).any(|(selected, home)| selected.1 != home.1)
		{
			return Err(());
		}

		Ok(Self { path: path.to_owned(), components })
	}

	fn revalidate(&self) -> Result<(), ()> {
		for (directory, expected) in &self.components {
			let metadata = directory.metadata().map_err(|_| ())?;
			if !metadata.is_dir() || DirectoryIdentity::from_metadata(&metadata) != *expected {
				return Err(());
			}
		}
		let current = Self::acquire(&self.path)?;
		if current.components.len() != self.components.len()
			|| current
				.components
				.iter()
				.zip(&self.components)
				.any(|(current, expected)| current.1 != expected.1)
		{
			return Err(());
		}
		Ok(())
	}

	fn descriptor(&self) -> i32 {
		self.components.last().expect("selected directory exists").0.as_raw_fd()
	}
}

fn open_selected_path(
	path: &Path,
	effective_uid: u32,
) -> Result<Vec<(File, DirectoryIdentity)>, ()> {
	let parts = selected_cwd_components(path)?;
	let root = open_selected_directory(Path::new("/"))?;
	let root_metadata = root.metadata().map_err(|_| ())?;
	validate_selected_component(&root_metadata, effective_uid, false)?;
	let mut components = vec![(root, DirectoryIdentity::from_metadata(&root_metadata))];

	for part in parts {
		let parent = &components.last().expect("root descriptor exists").0;
		let child = open_selected_directory_at(parent, part)?;
		let metadata = child.metadata().map_err(|_| ())?;
		validate_selected_component(&metadata, effective_uid, false)?;
		components.push((child, DirectoryIdentity::from_metadata(&metadata)));
	}

	let final_metadata =
		components.last().expect("selected directory exists").0.metadata().map_err(|_| ())?;
	validate_selected_component(&final_metadata, effective_uid, true)?;
	Ok(components)
}

impl QuickTaskPreSpawnCheck for SelectedWorkingDirectory {
	fn validate_at_spawn_boundary(&self) -> Result<(), ()> {
		self.revalidate()
	}

	fn working_directory_descriptor(&self) -> i32 {
		self.descriptor()
	}
}

fn effective_user_home(effective_uid: u32) -> Result<PathBuf, ()> {
	let mut buffer_len = INITIAL_PASSWD_BUFFER_BYTES;
	loop {
		let mut entry = MaybeUninit::<libc::passwd>::zeroed();
		let mut result = ptr::null_mut();
		let mut buffer = vec![0_u8; buffer_len];
		let status = unsafe {
			libc::getpwuid_r(
				effective_uid,
				entry.as_mut_ptr(),
				buffer.as_mut_ptr().cast(),
				buffer.len(),
				&mut result,
			)
		};
		if status == libc::ERANGE && buffer_len < MAX_PASSWD_BUFFER_BYTES {
			buffer_len = buffer_len.saturating_mul(2).min(MAX_PASSWD_BUFFER_BYTES);
			continue;
		}
		if status != 0 || result.is_null() {
			return Err(());
		}
		let entry = unsafe { entry.assume_init() };
		if entry.pw_dir.is_null() {
			return Err(());
		}
		let value = unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes();
		if value.is_empty() {
			return Err(());
		}
		return Ok(PathBuf::from(OsStr::from_bytes(value)));
	}
}

fn selected_cwd_components(path: &Path) -> Result<Vec<&OsStr>, ()> {
	let mut components = path.components();
	if !matches!(components.next(), Some(Component::RootDir)) {
		return Err(());
	}
	let mut parts = Vec::new();
	for component in components {
		match component {
			Component::Normal(part) if !part.is_empty() => parts.push(part),
			_ => return Err(()),
		}
	}
	if parts.is_empty() || parts.len() > MAX_SELECTED_CWD_COMPONENTS {
		return Err(());
	}
	Ok(parts)
}

fn validate_selected_component(
	metadata: &Metadata,
	effective_uid: u32,
	require_effective_owner: bool,
) -> Result<(), ()> {
	let owner_allowed = if require_effective_owner {
		metadata.uid() == effective_uid
	} else {
		metadata.uid() == 0 || metadata.uid() == effective_uid
	};
	if !metadata.is_dir() || !owner_allowed || metadata.mode() & 0o022 != 0 || metadata.nlink() == 0
	{
		return Err(());
	}
	Ok(())
}

fn open_selected_directory(path: &Path) -> Result<File, ()> {
	let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ())?;
	let descriptor = unsafe {
		libc::open(
			path.as_ptr(),
			libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
		)
	};
	selected_directory_from_descriptor(descriptor)
}

fn open_selected_directory_at(parent: &File, name: &OsStr) -> Result<File, ()> {
	let name = CString::new(name.as_bytes()).map_err(|_| ())?;
	let descriptor = unsafe {
		libc::openat(
			parent.as_raw_fd(),
			name.as_ptr(),
			libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
		)
	};
	selected_directory_from_descriptor(descriptor)
}

fn selected_directory_from_descriptor(descriptor: i32) -> Result<File, ()> {
	if descriptor == -1 {
		return Err(());
	}
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

/// Explicit request-scoped execution settings carried on every user send.
#[derive(Clone)]
pub(crate) struct QuickTaskExecutionSettings {
	pub model: String,
	pub reasoning_effort: String,
	pub fast: bool,
}

/// First ordinary Turn input. Settings are explicit and survive pre-session recovery.
pub(crate) struct CreateQuickTask {
	pub operation_key: String,
	pub correlation_id: String,
	pub causation_id: Option<String>,
	pub conversation_id: ConversationId,
	pub message: String,
	pub working_directory: String,
	pub execution: QuickTaskExecutionSettings,
}

/// Public recovery coordinates. Message, directory, and route authority come from durable state.
pub(crate) struct RecoverQuickTask {
	pub operation_key: String,
	pub correlation_id: String,
	pub causation_id: Option<String>,
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
}

/// Later ordinary Turn input over the exact existing RuntimeSession.
pub(crate) struct SubmitQuickTaskTurn {
	pub operation_key: String,
	pub correlation_id: String,
	pub causation_id: Option<String>,
	pub conversation_id: ConversationId,
	pub turn_id: TurnId,
	pub message: String,
	pub working_directory: String,
	pub execution: QuickTaskExecutionSettings,
}

/// Explicit selected-thread reconciliation request.
pub(crate) struct ControlQuickTask {
	pub operation_key: String,
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
	pub active_turn_id: Option<TurnId>,
	pub active_turn_revision: Option<i64>,
	pub archive: bool,
}

/// Closed selected-thread control result.
pub(crate) enum QuickTaskControlOutcome {
	Current,
	Archived { conversation_revision: i64 },
	Busy,
	Conflict,
	OutcomeUnknown,
	Unavailable,
}

struct InitialQuickTaskExecution {
	operation_key: String,
	correlation_id: String,
	causation_id: Option<String>,
	conversation_id: ConversationId,
	turn_id: TurnId,
	message: String,
	working_directory: String,
	execution: QuickTaskExecutionSettings,
}

/// Daemon-local projection. It is never serialized or accepted as durable authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuickTaskReadback {
	pub operation_key: Option<String>,
	pub correlation_id: Option<String>,
	pub causation_id: Option<String>,
	pub conversation_id: ConversationId,
	pub conversation_revision: Option<i64>,
	pub runtime_session_id: Option<RuntimeSessionId>,
	pub runtime_session_revision: Option<i64>,
	pub codex_thread_id: Option<String>,
	pub process_generation_id: Option<ProcessGenerationId>,
	pub active_turn_id: Option<TurnId>,
	pub state: QuickTaskLocalState,
}

/// Closed local lifecycle projection with no durable transition power.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskLocalState {
	RoutingPending,
	EstablishmentPending,
	QuotaExhausted,
	NoRoute,
	Establishing,
	Ready,
	Running,
	ManualRecovery,
	OutcomeUnknown,
}

/// Bounded daemon-local overlay for one durable ordinary Conversation projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuickTaskProjection {
	pub readback: QuickTaskReadback,
	pub recovery: Option<QuickTaskManualRecovery>,
}

/// Typed manual action after definite missing or incompatible authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskManualRecovery {
	EnableAccount,
	EnrollCredentials,
	ResolveAccountOperation,
	RepairCredentialStore,
	RestoreProviderAgreement,
	RefreshQuota,
	SelectedAccountDrift,
	SelectedAccountReadiness,
	UpgradeCodex,
	SelectWorkingDirectory,
	MissingLocalProcess,
	MissingThread,
	IncompatibleThread,
	PriorActiveTurn,
	PriorAttemptUnresolved,
	ProcessUnavailable,
}

/// Closed ambiguity location. None permits retry, search, or thread adoption.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskAmbiguity {
	ProcessGeneration,
	ThreadStart,
	ThreadBind,
	ThreadResume,
	TurnStart,
	ActiveTurn,
	TurnFinalization,
}

/// Positive terminal state retained from exact provider evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskTerminalState {
	Succeeded,
	Failed,
}

/// Complete application-facing synchronous results and asynchronous events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskOutcome {
	PreSession(QuickTaskReadback),
	Started {
		readback: QuickTaskReadback,
		provider_turn_id: String,
	},
	Streaming {
		readback: QuickTaskReadback,
		history_item_id: HistoryItemId,
		text: HistoryText,
	},
	Terminal {
		readback: QuickTaskReadback,
		turn_id: TurnId,
		state: QuickTaskTerminalState,
		provider_turn_id: String,
	},
	Unknown {
		readback: QuickTaskReadback,
		ambiguity: QuickTaskAmbiguity,
	},
	ManualRecovery {
		readback: QuickTaskReadback,
		action: QuickTaskManualRecovery,
	},
	Busy(QuickTaskReadback),
	Conflict,
	InterruptRequested(QuickTaskReadback),
	Unavailable,
}

/// Typed Quick Task readiness projected by daemon bootstrap and Doctor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskReadiness {
	/// Every fallible dependency owner returned one validated dependency.
	Ready,
	/// Quick Task is closed for one redacted startup reason.
	Unavailable(QuickTaskUnavailableReason),
}

/// Immutable daemon-lifetime Quick Task capability.
#[derive(Clone)]
pub(crate) enum QuickTaskCapability {
	Ready(QuickTaskRuntime),
	Unavailable(QuickTaskUnavailableReason),
}

impl QuickTaskCapability {
	pub(crate) const fn readiness(&self) -> QuickTaskReadiness {
		match self {
			Self::Ready(_) => QuickTaskReadiness::Ready,
			Self::Unavailable(reason) => QuickTaskReadiness::Unavailable(*reason),
		}
	}

	pub(crate) const fn runtime(&self) -> Option<&QuickTaskRuntime> {
		match self {
			Self::Ready(runtime) => Some(runtime),
			Self::Unavailable(_) => None,
		}
	}
}

/// Ordinary use-case composition. It owns no durable Quick Task state.
#[derive(Clone)]
pub(crate) struct QuickTaskRuntime {
	inner: Arc<QuickTaskRuntimeInner>,
}

struct QuickTaskRuntimeInner {
	store: SqliteStore,
	blob_store: decodex_core::BlobStore,
	accounts: Arc<AccountService>,
	process_generations: ProcessGenerationControl,
	provider_attempts: ProviderAttemptControl,
	execution_authorization: ProcessExecutionAuthorization,
	launch_profile: AttestedAppServerProfile,
	capacity: Arc<RunnerCapacity>,
	local: Mutex<BTreeMap<String, LocalTask>>,
	events: tokio_mpsc::Sender<QuickTaskOutcome>,
	event_receiver: AsyncMutex<tokio_mpsc::Receiver<QuickTaskOutcome>>,
	event_stream_closed: tokio::sync::watch::Sender<bool>,
	workers: AsyncMutex<JoinSet<()>>,
	shutting_down: Arc<std::sync::atomic::AtomicBool>,
}

struct LocalTask {
	operation_key: String,
	state: LocalTaskState,
}

enum LocalTaskState {
	Establishing,
	Preparing(LocalSession),
	Ready(LocalSession),
	Active {
		session: LocalSession,
		turn_id: TurnId,
		attempt_id: ProviderAttemptId,
		commands: mpsc::SyncSender<WorkerCommand>,
	},
	Recovery {
		readback: QuickTaskReadback,
		action: QuickTaskManualRecovery,
	},
}

#[derive(Clone)]
struct LocalSession {
	operation_key: String,
	correlation_id: String,
	causation_id: Option<String>,
	conversation_id: ConversationId,
	conversation_revision: i64,
	runtime_session_id: RuntimeSessionId,
	runtime_session_revision: i64,
	codex_thread_id: String,
	has_acknowledged_turn: bool,
	account_id: AccountId,
	process: FencedProcess,
	model: String,
	reasoning_effort: String,
	fast: bool,
	working_directory: String,
	instructions: String,
	next_user_sequence: i64,
}

enum WorkerCommand {
	Interrupt,
	Shutdown,
}

enum WorkerOutput {
	Event(QuickTaskProcessEvent),
	Failed,
}

enum ReservedTurnRefusal {
	Conflict,
	Unavailable,
	Recovery(QuickTaskManualRecovery),
}

enum PreparedCancellationDisposition {
	Canceled,
	Conflict,
	Ambiguous,
}

enum ExistingSessionPlanningRefusal {
	Recovery(QuickTaskManualRecovery),
	Conflict,
	Unknown,
}

const fn continuation_recovery(
	rejection: ContinuationRejection,
) -> Option<QuickTaskManualRecovery> {
	match rejection {
		ContinuationRejection::SelectedAccountDrift =>
			Some(QuickTaskManualRecovery::SelectedAccountDrift),
		ContinuationRejection::SelectedAccountReadinessRequired =>
			Some(QuickTaskManualRecovery::SelectedAccountReadiness),
		ContinuationRejection::SelectedAccountQuotaRequired =>
			Some(QuickTaskManualRecovery::RefreshQuota),
		_ => None,
	}
}

struct ExistingSessionExpectation<'a> {
	account_id: &'a AccountId,
	runtime_session_id: &'a RuntimeSessionId,
	runtime_session_revision: i64,
	thread_id: &'a str,
}

struct ExistingSessionPlanningInput<'a> {
	operation_key: &'a str,
	consumer: ExecutionConsumer,
	message_bytes: usize,
	expected: ExistingSessionExpectation<'a>,
}

enum SameThreadResumeRefusal {
	MissingThread,
	IncompatibleThread,
	ProcessUnavailable,
	Ambiguous,
}

#[derive(Clone)]
struct TurnContext {
	session: LocalSession,
	logical_turn_id: TurnId,
	logical_turn_sequence: i64,
	assistant_turn_id: TurnId,
	attempt_id: ProviderAttemptId,
	request_id: ProviderRequestId,
	provider_key: ProviderRequestKey,
	provider_turn_id: String,
	authorized_revision: i64,
}

struct AdmittedLaterTurn {
	session: LocalSession,
	sequence: i64,
	consumer: ExecutionConsumer,
}

struct RehydratedTurnAdmission {
	readback: OrdinaryRuntimeSessionResumeReadback,
	durable_readback: QuickTaskReadback,
	sequence: i64,
}

struct RehydratedTurnPlan {
	admission: RehydratedTurnAdmission,
	decision: PersistedDecisionProvenance,
	plan: decodex_database::ContinuationPlanEffect,
}

struct RehydratedAccountRevision {
	planned: RehydratedTurnPlan,
	launch_account_revision: i64,
}

struct RehydratedProcessAdmission {
	account: RehydratedAccountRevision,
	working_directory: String,
	admission: FreshQuickTaskProcessGeneration,
	establishment: ReconcileQuickTaskThreadEstablishment,
}

struct RehydratedProcessLaunch {
	decision: PersistedDecisionProvenance,
	plan: decodex_database::ContinuationPlanEffect,
	session: LocalSession,
	sequence: i64,
}

enum InitialRouteAction {
	Route,
	ResumeEstablishment,
	Preplanned(Box<PreProcessOutcome>),
}

impl QuickTaskRuntime {
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new(
		store: SqliteStore,
		blob_store: decodex_core::BlobStore,
		accounts: Arc<AccountService>,
		process_generations: ProcessGenerationControl,
		provider_attempts: ProviderAttemptControl,
		execution_authorization: ProcessExecutionAuthorization,
		launch_profile: AttestedAppServerProfile,
		capacity: Arc<RunnerCapacity>,
	) -> Self {
		let (events, event_receiver) = tokio_mpsc::channel(EVENT_QUEUE_CAPACITY);
		let (event_stream_closed, _) = tokio::sync::watch::channel(false);
		Self {
			inner: Arc::new(QuickTaskRuntimeInner {
				store,
				blob_store,
				accounts,
				process_generations,
				provider_attempts,
				execution_authorization,
				launch_profile,
				capacity,
				local: Mutex::new(BTreeMap::new()),
				events,
				event_receiver: AsyncMutex::new(event_receiver),
				event_stream_closed,
				workers: AsyncMutex::new(JoinSet::new()),
				shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
			}),
		}
	}

	pub(crate) async fn create(&self, command: CreateQuickTask) -> QuickTaskOutcome {
		if self.is_shutting_down() {
			return QuickTaskOutcome::Unavailable;
		}
		if let Err(outcome) = self.reserve_initial(&command) {
			return *outcome;
		}
		let conversation_id = command.conversation_id.clone();
		let outcome = self.create_inner(command).await;
		if matches!(
			&outcome,
			QuickTaskOutcome::PreSession(_)
				| QuickTaskOutcome::Unavailable
				| QuickTaskOutcome::Conflict
		) {
			self.remove_establishing(&conversation_id);
		}
		outcome
	}

	/// Resume the sole initial route from product-store-owned request coordinates.
	pub(crate) async fn resume_routing(&self, command: RecoverQuickTask) -> QuickTaskOutcome {
		self.resume_initial(command, InitialRouteAction::Route).await
	}

	/// Resume only first-session planning from one committed selected decision.
	pub(crate) async fn resume_establishment(&self, command: RecoverQuickTask) -> QuickTaskOutcome {
		self.resume_initial(command, InitialRouteAction::ResumeEstablishment).await
	}

	/// Start lifecycle work from an already routed successor without invoking routing persistence.
	pub(crate) async fn start_preplanned_initial(
		&self,
		command: RecoverQuickTask,
		routing: PreProcessOutcome,
	) {
		if self.is_shutting_down() || command.expected_conversation_revision <= 0 {
			return;
		}
		let request = match self.inner.store.read_quick_task_request(&command.conversation_id).await
		{
			Ok(Some(request)) => request,
			Ok(None) | Err(_) => return,
		};
		let initial = CreateQuickTask {
			operation_key: command.operation_key,
			correlation_id: command.correlation_id,
			causation_id: command.causation_id,
			conversation_id: command.conversation_id,
			message: request.message,
			working_directory: request.working_directory,
			execution: QuickTaskExecutionSettings {
				model: request.model,
				reasoning_effort: request.reasoning_effort,
				fast: request.fast,
			},
		};
		let _ = self
			.run_reserved_initial(
				initial,
				command.expected_conversation_revision,
				InitialRouteAction::Preplanned(Box::new(routing)),
			)
			.await;
	}

	async fn resume_initial(
		&self,
		command: RecoverQuickTask,
		action: InitialRouteAction,
	) -> QuickTaskOutcome {
		if self.is_shutting_down() || command.expected_conversation_revision <= 0 {
			return QuickTaskOutcome::Unavailable;
		}
		let request = match self.inner.store.read_quick_task_request(&command.conversation_id).await
		{
			Ok(Some(request)) => request,
			Ok(None) | Err(_) => return QuickTaskOutcome::Unavailable,
		};
		self.run_reserved_initial(
			CreateQuickTask {
				operation_key: command.operation_key,
				correlation_id: command.correlation_id,
				causation_id: command.causation_id,
				conversation_id: command.conversation_id,
				message: request.message,
				working_directory: request.working_directory,
				execution: QuickTaskExecutionSettings {
					model: request.model,
					reasoning_effort: request.reasoning_effort,
					fast: request.fast,
				},
			},
			command.expected_conversation_revision,
			action,
		)
		.await
	}

	async fn run_reserved_initial(
		&self,
		command: CreateQuickTask,
		conversation_revision: i64,
		action: InitialRouteAction,
	) -> QuickTaskOutcome {
		if let Err(outcome) = self.reserve_initial(&command) {
			return *outcome;
		}
		let conversation_id = command.conversation_id.clone();
		let outcome = self.establish_first_session(command, conversation_revision, action).await;
		if matches!(
			&outcome,
			QuickTaskOutcome::PreSession(_)
				| QuickTaskOutcome::Unavailable
				| QuickTaskOutcome::Conflict
		) {
			self.remove_establishing(&conversation_id);
		}
		outcome
	}

	#[allow(clippy::too_many_lines)]
	async fn create_inner(&self, command: CreateQuickTask) -> QuickTaskOutcome {
		let conversation_command = match exact_command(
			"conversation",
			&command.operation_key,
			&[
				command.conversation_id.as_str(),
				"Quick Task",
				&command.message,
				&command.working_directory,
				&command.execution.model,
				&command.execution.reasoning_effort,
				if command.execution.fast { "priority" } else { "default" },
			],
		) {
			Ok(command) => command,
			Err(()) => return QuickTaskOutcome::Conflict,
		};
		let conversation = match self
			.inner
			.store
			.create_quick_task_conversation(
				&conversation_command,
				&CreateQuickTaskConversation {
					conversation_id: command.conversation_id.clone(),
					title: "Quick Task".to_owned(),
					message: command.message.clone(),
					working_directory: command.working_directory.clone(),
					model: command.execution.model.clone(),
					reasoning_effort: command.execution.reasoning_effort.clone(),
					fast: command.execution.fast,
				},
			)
			.await
		{
			Ok(conversation) => conversation,
			Err(error) => return store_outcome(error),
		};
		self.establish_first_session(command, conversation.revision, InitialRouteAction::Route)
			.await
	}

	#[allow(clippy::too_many_lines)]
	async fn establish_first_session(
		&self,
		command: CreateQuickTask,
		conversation_revision: i64,
		action: InitialRouteAction,
	) -> QuickTaskOutcome {
		let outcome = match action {
			InitialRouteAction::Route => {
				let execution = ExecutionCommand::initial_thread(
					&command.operation_key,
					command.conversation_id.clone(),
					conversation_revision,
				);
				ExecutionCoordinator.pre_process(&self.inner.store, &execution).await
			},
			InitialRouteAction::ResumeEstablishment =>
				ExecutionCoordinator
					.resume_establishment(&self.inner.store, &command.conversation_id)
					.await,
			InitialRouteAction::Preplanned(outcome) => *outcome,
		};
		let (decision, plan) = match outcome {
			PreProcessOutcome::Planned { decision, plan } => (decision, plan),
			PreProcessOutcome::Waiting => {
				return self.pre_session(
					&command,
					conversation_revision,
					QuickTaskLocalState::QuotaExhausted,
				);
			},
			PreProcessOutcome::EstablishmentPending => {
				return self.pre_session(
					&command,
					conversation_revision,
					QuickTaskLocalState::EstablishmentPending,
				);
			},
			PreProcessOutcome::NoRoute => {
				return self.pre_session(
					&command,
					conversation_revision,
					QuickTaskLocalState::NoRoute,
				);
			},
			PreProcessOutcome::FailedClosed(_) => {
				return self.pre_session(
					&command,
					conversation_revision,
					QuickTaskLocalState::RoutingPending,
				);
			},
		};
		let consumer = decision.consumer.clone();
		let turn_id = match &consumer {
			ExecutionConsumer::ConversationTurn {
				conversation_id,
				conversation_revision: revision,
				source_runtime_session_id: None,
				source_runtime_session_revision: None,
				turn_id,
			} if conversation_id == &command.conversation_id
				&& *revision == conversation_revision =>
				turn_id.clone(),
			_ => return QuickTaskOutcome::Conflict,
		};
		let command = InitialQuickTaskExecution {
			operation_key: scoped_key("initial-lifecycle", &decision.decision_id),
			correlation_id: command.correlation_id,
			causation_id: command.causation_id,
			conversation_id: command.conversation_id,
			turn_id,
			message: command.message,
			working_directory: command.working_directory,
			execution: command.execution,
		};
		let working_directory = command.working_directory.clone();
		let Some(session) = plan.runtime_session.clone() else {
			return QuickTaskOutcome::Conflict;
		};
		if decision.consumer != consumer
			|| plan.plan.consumer != consumer
			|| plan.plan.kind != decodex_core::ContinuationPlanKind::InitialThread
			|| plan.plan.source_runtime_session_id != session.runtime_session_id
			|| plan.plan.source_runtime_session_revision != session.revision
			|| session.conversation_id != command.conversation_id
			|| session.revision != 1
			|| session.state != RuntimeSessionState::Starting
			|| session.codex_thread_id.is_some()
			|| session.last_known_turn_id.is_some()
			|| session.ended_at.is_some()
			|| session.profile_snapshot.role != RoleProfileRole::Task
			|| session.account_snapshot.source_account_id != plan.plan.selected_account_id
			|| session.account_snapshot.source_revision <= 0
		{
			return QuickTaskOutcome::Conflict;
		}
		let runtime_session_id = session.runtime_session_id.clone();
		let selected_account_id = plan.plan.selected_account_id.clone();
		let selected_account_revision = session.account_snapshot.source_revision;
		let history_item_id = match HistoryItemId::new(derived_uuid(
			"user-history",
			&[command.operation_key.as_str(), command.turn_id.as_str()],
		)) {
			Ok(value) => value,
			Err(_) => return QuickTaskOutcome::Conflict,
		};
		let turn_admission = match self
			.inner
			.store
			.admit_initial_quick_task_turn(
				&self.inner.blob_store,
				&scoped_key("initial-turn-admission", &command.operation_key),
				&AdmitInitialQuickTaskTurn {
					expected_conversation_revision: conversation_revision,
					expected_runtime_session_revision: session.revision,
					continuation_plan_id: plan.plan.plan_id.clone(),
					message: RecordHistoryItem {
						conversation_id: command.conversation_id.clone(),
						runtime_session_id: runtime_session_id.clone(),
						turn_id: command.turn_id.clone(),
						turn_sequence: 1,
						turn_role: TurnRole::User,
						possible_side_effects: PossibleSideEffects::Unknown,
						history_item_id: history_item_id.clone(),
						ordinal: 0,
						kind: HistoryItemKind::Message,
						status: ItemStatus::Completed,
						text: command.message.clone(),
						media_type: markdown_media_type(),
						metadata: HistoryMetadata::empty(),
						expected_revision: None,
						artifact: None,
					},
				},
			)
			.await
		{
			Ok(
				InitialQuickTaskTurnAdmissionOutcome::Fresh(admission)
				| InitialQuickTaskTurnAdmissionOutcome::Replayed(admission),
			) if admission.routing_decision_id == decision.decision_id
				&& admission.continuation_plan_id == plan.plan.plan_id
				&& admission.history_item_id == history_item_id
				&& admission.turn.turn_id == command.turn_id
				&& admission.turn.sequence == 1
				&& admission.turn.status == decodex_core::TurnStatus::Active
				&& admission.turn.revision == 1 =>
				admission,
			Ok(_) => return QuickTaskOutcome::Conflict,
			Err(error)
				if turn_reservation_is_definite(&error)
					|| turn_reservation_is_integrity_failure(&error) =>
			{
				return QuickTaskOutcome::Conflict;
			},
			Err(_) => {
				return self
					.ambiguous(
						QuickTaskReadback {
							operation_key: Some(command.operation_key.clone()),
							correlation_id: Some(command.correlation_id.clone()),
							causation_id: command.causation_id.clone(),
							conversation_id: command.conversation_id.clone(),
							conversation_revision: Some(conversation_revision),
							runtime_session_id: Some(runtime_session_id.clone()),
							runtime_session_revision: Some(session.revision),
							codex_thread_id: None,
							process_generation_id: None,
							active_turn_id: Some(command.turn_id.clone()),
							state: QuickTaskLocalState::OutcomeUnknown,
						},
						QuickTaskAmbiguity::TurnFinalization,
					)
					.await;
			},
		};
		if turn_admission.turn.status != decodex_core::TurnStatus::Active
			|| turn_admission.turn.revision != 1
		{
			return QuickTaskOutcome::Conflict;
		}
		let created = QuickTaskReadback {
			operation_key: Some(command.operation_key.clone()),
			correlation_id: Some(command.correlation_id.clone()),
			causation_id: command.causation_id.clone(),
			conversation_id: command.conversation_id.clone(),
			conversation_revision: Some(conversation_revision),
			runtime_session_id: Some(runtime_session_id.clone()),
			runtime_session_revision: Some(session.revision),
			codex_thread_id: None,
			process_generation_id: None,
			active_turn_id: Some(command.turn_id.clone()),
			state: QuickTaskLocalState::Establishing,
		};
		let generation_id = match ProcessGenerationId::new(derived_uuid(
			"process-generation",
			&[command.operation_key.as_str(), command.conversation_id.as_str()],
		)) {
			Ok(value) => value,
			Err(_) => {
				return self
					.finalize_initial_refusal(
						&command.operation_key,
						&command.turn_id,
						created,
						ReservedTurnRefusal::Unavailable,
					)
					.await;
			},
		};
		let process_request = PrepareQuickTaskProcessGeneration {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: conversation_revision,
			runtime_session_id: runtime_session_id.clone(),
			expected_runtime_session_revision: session.revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: plan.plan.plan_id.clone(),
			routing_decision_id: decision.decision_id.clone(),
			selected_account_id: selected_account_id.clone(),
			process_generation_id: generation_id.clone(),
		};
		let establishment = ReconcileQuickTaskThreadEstablishment {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: conversation_revision,
			runtime_session_id: runtime_session_id.clone(),
			expected_runtime_session_revision: session.revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: plan.plan.plan_id.clone(),
			routing_decision_id: decision.decision_id.clone(),
			selected_account_id: selected_account_id.clone(),
			process_generation_id: generation_id,
		};
		let admission = match self
			.inner
			.store
			.prepare_quick_task_process_generation(
				&scoped_key("process-admission", &command.operation_key),
				&process_request,
			)
			.await
		{
			Ok(PrepareQuickTaskProcessGenerationOutcome::Fresh(admission)) => admission,
			Ok(PrepareQuickTaskProcessGenerationOutcome::Rejected(_)) => {
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						created,
						&establishment,
						ReservedTurnRefusal::Conflict,
					)
					.await;
			},
			Ok(
				PrepareQuickTaskProcessGenerationOutcome::Replayed(_)
				| PrepareQuickTaskProcessGenerationOutcome::Unknown(_),
			)
			| Err(_) => {
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						created,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable),
					)
					.await;
			},
		};
		let process = match self
			.launch_process(
				&selected_account_id,
				selected_account_revision,
				admission,
				&working_directory,
			)
			.await
		{
			Ok(process) => process,
			Err(action) => {
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						created,
						&establishment,
						ReservedTurnRefusal::Recovery(action),
					)
					.await;
			},
		};
		let spawned = QuickTaskReadback {
			process_generation_id: Some(process.generation_id().clone()),
			..created.clone()
		};
		let request = match QuickTaskThreadStartRequest::new(
			command.execution.model.clone(),
			working_directory.clone(),
			session.profile_snapshot.instructions.clone(),
		)
		.map(|request| request.with_fast(command.execution.fast))
		{
			Ok(request) => request,
			Err(_) => {
				self.terminate_process(&process).await;
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						spawned,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::IncompatibleThread),
					)
					.await;
			},
		};
		let prepared = match self.prepare_thread_start(&process, request).await {
			Ok(prepared) => prepared,
			Err(_) => {
				self.terminate_process(&process).await;
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						spawned,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable),
					)
					.await;
			},
		};
		let fence_key = scoped_key("thread-fence", &command.operation_key);
		let fence = FenceRuntimeSessionThreadStart {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: conversation_revision,
			runtime_session_id: runtime_session_id.clone(),
			expected_revision: session.revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: plan.plan.plan_id.clone(),
			process_generation_id: process.generation_id().clone(),
			process_generation_revision: process.revision(),
			process_execution_epoch_id: self.inner.execution_authorization.epoch_id.clone(),
			thread_start_request_id: prepared.request_id(),
			thread_start_request_sha256: prepared.request_sha256().to_owned(),
		};
		let authority =
			match self.inner.store.fence_runtime_session_thread_start(&fence_key, &fence).await {
				Ok(FenceRuntimeSessionThreadStartOutcome::Fresh(authority)) => authority,
				Ok(
					FenceRuntimeSessionThreadStartOutcome::Replayed(_)
					| FenceRuntimeSessionThreadStartOutcome::Rejected(_),
				)
				| Err(_) => {
					self.terminate_process(&process).await;
					return self
						.reconcile_pre_effect(
							&command.operation_key,
							&command.turn_id,
							spawned,
							&establishment,
							ReservedTurnRefusal::Conflict,
						)
						.await;
				},
			};
		let fence_readback = authority.readback();
		if fence_readback.conversation_id != command.conversation_id
			|| fence_readback.conversation_revision != conversation_revision
			|| fence_readback.runtime_session_id != runtime_session_id
			|| fence_readback.prior_revision != session.revision
			|| session.revision.checked_add(1) != Some(fence_readback.revision)
			|| fence_readback.turn_id != command.turn_id
			|| fence_readback.turn_revision != 1
			|| fence_readback.continuation_plan_id != plan.plan.plan_id
			|| fence_readback.routing_decision_id != decision.decision_id
			|| fence_readback.selected_account_id != selected_account_id
			|| &fence_readback.process_generation_id != process.generation_id()
			|| fence_readback.process_generation_revision != process.revision()
			|| fence_readback.process_execution_epoch_id
				!= self.inner.execution_authorization.epoch_id
			|| fence_readback.thread_start_request_id != prepared.request_id()
			|| fence_readback.thread_start_request_sha256 != prepared.request_sha256()
		{
			self.terminate_process(&process).await;
			return self.ambiguous(spawned, QuickTaskAmbiguity::ThreadStart).await;
		}
		let fenced = QuickTaskReadback {
			operation_key: Some(command.operation_key.clone()),
			correlation_id: Some(command.correlation_id.clone()),
			causation_id: command.causation_id.clone(),
			conversation_id: command.conversation_id.clone(),
			conversation_revision: Some(conversation_revision),
			runtime_session_id: Some(runtime_session_id.clone()),
			runtime_session_revision: Some(fence_readback.revision),
			codex_thread_id: None,
			process_generation_id: Some(process.generation_id().clone()),
			active_turn_id: Some(command.turn_id.clone()),
			state: QuickTaskLocalState::Establishing,
		};
		let established = match self.start_thread(&process, prepared, authority).await {
			Ok(established) => established,
			Err(_) => {
				self.terminate_process(&process).await;
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						fenced,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable),
					)
					.await;
			},
		};
		if !established.events.is_empty() {
			self.terminate_process(&process).await;
			return self.ambiguous(fenced, QuickTaskAmbiguity::ThreadStart).await;
		}
		let binding_key = scoped_key("thread-bind", &command.operation_key);
		let binding = match self
			.inner
			.store
			.bind_runtime_session_thread(&binding_key, &established.binding)
			.await
		{
			Ok(BindRuntimeSessionThreadOutcome::Applied(binding))
			| Ok(BindRuntimeSessionThreadOutcome::Replayed(binding)) => binding,
			Ok(BindRuntimeSessionThreadOutcome::Rejected(_)) | Err(_) => match self
				.inner
				.store
				.reconcile_quick_task_thread_establishment(&establishment)
				.await
			{
				Ok(QuickTaskThreadEstablishmentReadback::Bound(binding)) => binding,
				Ok(
					QuickTaskThreadEstablishmentReadback::Fenced(_)
					| QuickTaskThreadEstablishmentReadback::DefinitelyNotStarted(_)
					| QuickTaskThreadEstablishmentReadback::Unknown,
				)
				| Err(_) => {
					self.terminate_process(&process).await;
					return self.ambiguous(fenced, QuickTaskAmbiguity::ThreadBind).await;
				},
			},
		};
		if binding.conversation_id != command.conversation_id
			|| binding.conversation_revision != conversation_revision
			|| binding.runtime_session_id != runtime_session_id
			|| binding.prior_revision != established.binding.expected_revision
			|| binding.revision != established.binding.expected_revision.saturating_add(1)
			|| binding.turn_id != command.turn_id
			|| binding.turn_revision != 1
			|| binding.fence_prior_revision != session.revision
			|| binding.fence_revision != established.binding.expected_revision
			|| binding.continuation_plan_id != established.binding.continuation_plan_id
			|| binding.fence_idempotency_key != established.binding.fence_idempotency_key
			|| binding.binding_idempotency_key != binding_key
			|| binding.thread_start_request_id != established.binding.thread_start_request_id
			|| binding.thread_start_request_sha256
				!= established.binding.thread_start_request_sha256
			|| binding.thread_start_response_id
				!= established.binding.successful_response.response_id
			|| binding.thread_start_response_sha256
				!= established.binding.successful_response.response_sha256
			|| binding.codex_thread_id != established.codex_thread_id
			|| binding.codex_thread_id != established.binding.successful_response.codex_thread_id
		{
			self.terminate_process(&process).await;
			return self.ambiguous(fenced, QuickTaskAmbiguity::ThreadBind).await;
		}
		let local = LocalSession {
			operation_key: command.operation_key.clone(),
			correlation_id: command.correlation_id.clone(),
			causation_id: command.causation_id.clone(),
			conversation_id: command.conversation_id.clone(),
			conversation_revision,
			runtime_session_id,
			runtime_session_revision: binding.revision,
			codex_thread_id: binding.codex_thread_id.clone(),
			has_acknowledged_turn: false,
			account_id: selected_account_id,
			process,
			model: command.execution.model.clone(),
			reasoning_effort: command.execution.reasoning_effort.clone(),
			fast: command.execution.fast,
			working_directory,
			instructions: session.profile_snapshot.instructions,
			next_user_sequence: 2,
		};
		if !self.set_preparing(local.clone()) {
			self.terminate_process(&local.process).await;
			let readback = session_readback(
				&local,
				QuickTaskLocalState::OutcomeUnknown,
				Some(command.turn_id.clone()),
			);
			return self.ambiguous(readback, QuickTaskAmbiguity::ThreadBind).await;
		}
		self.dispatch_turn(
			&command.operation_key,
			decision,
			plan,
			local,
			command.turn_id,
			1,
			command.message,
			None,
			ProviderAttemptRuntimeAuthority::InitialSessionBinding {
				binding,
				process_execution_epoch_id: self.inner.execution_authorization.epoch_id.clone(),
			},
		)
		.await
	}

	#[allow(clippy::too_many_lines)]
	async fn establish_context_fallback(
		&self,
		command: SubmitQuickTaskTurn,
		turn_sequence: i64,
		decision: PersistedDecisionProvenance,
		plan: decodex_database::ContinuationPlanEffect,
		prior_session: Option<LocalSession>,
	) -> QuickTaskOutcome {
		let working_directory = command.working_directory.clone();
		let conversation_revision = plan.plan.consumer.domain_revision();
		let Some(runtime_session) = plan.runtime_session.clone() else {
			return QuickTaskOutcome::Conflict;
		};
		let Some(context_pack) =
			plan.fallback_context_pack.as_ref().map(|record| record.pack.clone())
		else {
			return QuickTaskOutcome::Conflict;
		};
		if plan.plan.kind != decodex_core::ContinuationPlanKind::ContextPackFallback
			|| plan.plan.consumer
				!= (ExecutionConsumer::ConversationTurn {
					conversation_id: command.conversation_id.clone(),
					conversation_revision,
					source_runtime_session_id: Some(plan.plan.source_runtime_session_id.clone()),
					source_runtime_session_revision: Some(
						plan.plan.source_runtime_session_revision,
					),
					turn_id: command.turn_id.clone(),
				}) || plan.plan.fallback_runtime_session_id.as_ref()
			!= Some(&runtime_session.runtime_session_id)
			|| runtime_session.conversation_id != command.conversation_id
			|| runtime_session.state != RuntimeSessionState::Starting
			|| runtime_session.revision != 1
			|| runtime_session.codex_thread_id.is_some()
			|| runtime_session.last_known_turn_id.is_some()
			|| runtime_session.profile_snapshot.role != RoleProfileRole::Task
			|| runtime_session.account_snapshot.source_account_id != plan.plan.selected_account_id
			|| context_pack.conversation_id() != &command.conversation_id
		{
			return QuickTaskOutcome::Conflict;
		}
		if let Some(prior) = &prior_session {
			self.terminate_process(&prior.process).await;
		}
		let runtime_session_id = runtime_session.runtime_session_id.clone();
		let selected_account_id = runtime_session.account_snapshot.source_account_id.clone();
		let created = QuickTaskReadback {
			operation_key: Some(command.operation_key.clone()),
			correlation_id: Some(command.correlation_id.clone()),
			causation_id: command.causation_id.clone(),
			conversation_id: command.conversation_id.clone(),
			conversation_revision: Some(conversation_revision),
			runtime_session_id: Some(runtime_session_id.clone()),
			runtime_session_revision: Some(runtime_session.revision),
			codex_thread_id: None,
			process_generation_id: None,
			active_turn_id: Some(command.turn_id.clone()),
			state: QuickTaskLocalState::Establishing,
		};
		let generation_id = ProcessGenerationId::new(derived_uuid(
			"fallback-process-generation",
			&[command.operation_key.as_str(), command.conversation_id.as_str()],
		))
		.expect("derived UUID-v4 is valid");
		let process_request = PrepareQuickTaskProcessGeneration {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: conversation_revision,
			runtime_session_id: runtime_session_id.clone(),
			expected_runtime_session_revision: runtime_session.revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: plan.plan.plan_id.clone(),
			routing_decision_id: decision.decision_id.clone(),
			selected_account_id: selected_account_id.clone(),
			process_generation_id: generation_id.clone(),
		};
		let establishment = ReconcileQuickTaskThreadEstablishment {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: conversation_revision,
			runtime_session_id: runtime_session_id.clone(),
			expected_runtime_session_revision: runtime_session.revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: plan.plan.plan_id.clone(),
			routing_decision_id: decision.decision_id.clone(),
			selected_account_id: selected_account_id.clone(),
			process_generation_id: generation_id,
		};
		let admission = match self
			.inner
			.store
			.prepare_quick_task_process_generation(
				&scoped_key("process-admission", &command.operation_key),
				&process_request,
			)
			.await
		{
			Ok(PrepareQuickTaskProcessGenerationOutcome::Fresh(admission)) => admission,
			Ok(PrepareQuickTaskProcessGenerationOutcome::Rejected(_)) => {
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						created,
						&establishment,
						ReservedTurnRefusal::Conflict,
					)
					.await;
			},
			Ok(
				PrepareQuickTaskProcessGenerationOutcome::Replayed(_)
				| PrepareQuickTaskProcessGenerationOutcome::Unknown(_),
			)
			| Err(_) => {
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						created,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable),
					)
					.await;
			},
		};
		let process = match self
			.launch_process(
				&selected_account_id,
				runtime_session.account_snapshot.source_revision,
				admission,
				&working_directory,
			)
			.await
		{
			Ok(process) => process,
			Err(action) => {
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						created,
						&establishment,
						ReservedTurnRefusal::Recovery(action),
					)
					.await;
			},
		};
		let spawned = QuickTaskReadback {
			process_generation_id: Some(process.generation_id().clone()),
			..created.clone()
		};
		let request = match QuickTaskThreadStartRequest::new(
			command.execution.model.clone(),
			working_directory.clone(),
			runtime_session.profile_snapshot.instructions.clone(),
		)
		.map(|request| request.with_fast(command.execution.fast))
		{
			Ok(request) => request,
			Err(_) => {
				self.terminate_process(&process).await;
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						spawned,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::IncompatibleThread),
					)
					.await;
			},
		};
		let prepared = match self.prepare_thread_start(&process, request).await {
			Ok(prepared) => prepared,
			Err(_) => {
				self.terminate_process(&process).await;
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						spawned,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable),
					)
					.await;
			},
		};
		let fence_key = scoped_key("thread-fence", &command.operation_key);
		let fence = FenceRuntimeSessionThreadStart {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: conversation_revision,
			runtime_session_id: runtime_session_id.clone(),
			expected_revision: runtime_session.revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: plan.plan.plan_id.clone(),
			process_generation_id: process.generation_id().clone(),
			process_generation_revision: process.revision(),
			process_execution_epoch_id: self.inner.execution_authorization.epoch_id.clone(),
			thread_start_request_id: prepared.request_id(),
			thread_start_request_sha256: prepared.request_sha256().to_owned(),
		};
		let authority =
			match self.inner.store.fence_runtime_session_thread_start(&fence_key, &fence).await {
				Ok(FenceRuntimeSessionThreadStartOutcome::Fresh(authority)) => authority,
				Ok(
					FenceRuntimeSessionThreadStartOutcome::Replayed(_)
					| FenceRuntimeSessionThreadStartOutcome::Rejected(_),
				)
				| Err(_) => {
					self.terminate_process(&process).await;
					return self
						.reconcile_pre_effect(
							&command.operation_key,
							&command.turn_id,
							spawned,
							&establishment,
							ReservedTurnRefusal::Recovery(
								QuickTaskManualRecovery::ProcessUnavailable,
							),
						)
						.await;
				},
			};
		let fence_readback = authority.readback();
		if fence_readback.conversation_id != command.conversation_id
			|| fence_readback.conversation_revision != conversation_revision
			|| fence_readback.runtime_session_id != runtime_session_id
			|| fence_readback.prior_revision != runtime_session.revision
			|| fence_readback.turn_id != command.turn_id
			|| fence_readback.turn_revision != 1
			|| fence_readback.continuation_plan_id != plan.plan.plan_id
			|| fence_readback.routing_decision_id != decision.decision_id
			|| fence_readback.selected_account_id != selected_account_id
			|| &fence_readback.process_generation_id != process.generation_id()
			|| fence_readback.process_generation_revision != process.revision()
		{
			self.terminate_process(&process).await;
			return self.ambiguous(spawned, QuickTaskAmbiguity::ThreadStart).await;
		}
		let fenced = QuickTaskReadback {
			runtime_session_revision: Some(fence_readback.revision),
			..spawned
		};
		let established = match self.start_thread(&process, prepared, authority).await {
			Ok(established) if established.events.is_empty() => established,
			Ok(_) | Err(_) => {
				self.terminate_process(&process).await;
				return self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						fenced,
						&establishment,
						ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable),
					)
					.await;
			},
		};
		let binding_key = scoped_key("thread-bind", &command.operation_key);
		let binding = match self
			.inner
			.store
			.bind_runtime_session_thread(&binding_key, &established.binding)
			.await
		{
			Ok(BindRuntimeSessionThreadOutcome::Applied(binding))
			| Ok(BindRuntimeSessionThreadOutcome::Replayed(binding)) => binding,
			Ok(BindRuntimeSessionThreadOutcome::Rejected(_)) | Err(_) => match self
				.inner
				.store
				.reconcile_quick_task_thread_establishment(&establishment)
				.await
			{
				Ok(QuickTaskThreadEstablishmentReadback::Bound(binding)) => binding,
				_ => {
					self.terminate_process(&process).await;
					return self.ambiguous(fenced, QuickTaskAmbiguity::ThreadBind).await;
				},
			},
		};
		if binding.conversation_id != command.conversation_id
			|| binding.conversation_revision != conversation_revision
			|| binding.runtime_session_id != runtime_session_id
			|| binding.turn_id != command.turn_id
			|| binding.turn_revision != 1
			|| binding.fence_prior_revision != runtime_session.revision
			|| binding.fence_revision.checked_add(1) != Some(binding.revision)
			|| binding.continuation_plan_id != plan.plan.plan_id
			|| binding.codex_thread_id != established.codex_thread_id
		{
			self.terminate_process(&process).await;
			return self.ambiguous(fenced, QuickTaskAmbiguity::ThreadBind).await;
		}
		let local = LocalSession {
			operation_key: command.operation_key.clone(),
			correlation_id: command.correlation_id.clone(),
			causation_id: command.causation_id.clone(),
			conversation_id: command.conversation_id.clone(),
			conversation_revision,
			runtime_session_id,
			runtime_session_revision: binding.revision,
			codex_thread_id: binding.codex_thread_id.clone(),
			has_acknowledged_turn: false,
			account_id: selected_account_id,
			process,
			model: command.execution.model.clone(),
			reasoning_effort: command.execution.reasoning_effort.clone(),
			fast: command.execution.fast,
			working_directory,
			instructions: runtime_session.profile_snapshot.instructions,
			next_user_sequence: turn_sequence.saturating_add(1),
		};
		let installed = match prior_session {
			Some(prior) => self.replace_preparing_successor(&prior, local.clone()),
			None => self
				.install_rehydrated_preparing(
					&command.operation_key,
					&command.conversation_id,
					&local,
				)
				.is_ok(),
		};
		if !installed {
			self.terminate_process(&local.process).await;
			return self
				.ambiguous(
					session_readback(
						&local,
						QuickTaskLocalState::OutcomeUnknown,
						Some(command.turn_id.clone()),
					),
					QuickTaskAmbiguity::ThreadBind,
				)
				.await;
		}
		self.dispatch_turn(
			&command.operation_key,
			decision,
			plan,
			local,
			command.turn_id,
			turn_sequence,
			command.message,
			Some(context_pack),
			ProviderAttemptRuntimeAuthority::FallbackSessionBinding {
				binding,
				process_execution_epoch_id: self.inner.execution_authorization.epoch_id.clone(),
			},
		)
		.await
	}

	async fn compile_fallback_context_pack(
		&self,
		conversation_id: &ConversationId,
		conversation_revision: i64,
		message_bytes: usize,
	) -> Result<ContextPack, StoreError> {
		let revision = u64::try_from(conversation_revision)
			.map_err(|_| StoreError::InvalidInput("Conversation revision is invalid"))?;
		let entries = self
			.inner
			.store
			.recent_conversation_history(
				&self.inner.blob_store,
				conversation_id,
				u16::try_from(MAX_CONTEXT_RECENT_ITEMS).unwrap_or(u16::MAX),
			)
			.await?;
		let mut optional_sources = Vec::with_capacity(entries.len());
		for entry in entries {
			let content = match (entry.inline_text, entry.blob_hash) {
				(Some(text), None) => text.into_bytes(),
				(None, Some(hash)) => self.inner.blob_store.read(hash)?,
				_ => {
					return Err(StoreError::Incompatible(
						"verified history payload shape is incomplete".into(),
					));
				},
			};
			let source_revision = u64::try_from(entry.revision)
				.map_err(|_| StoreError::Incompatible("history revision is invalid".into()))?;
			let source = match entry.artifact {
				Some((artifact_id, artifact_revision)) =>
					ContextPackSource::artifact(artifact_id, artifact_revision, content),
				None => ContextPackSource::new(
					ContextSourceKind::RecentRaw,
					entry.history_item_id,
					source_revision,
					content,
				),
			}
			.map_err(|_| StoreError::InvalidInput("history cannot compile into a Context Pack"))?;
			optional_sources.push(source);
		}
		let max_bytes = MAX_CONTEXT_PACK_BYTES.min(
			MAX_QUICK_TASK_INPUT_BYTES
				.checked_sub(message_bytes)
				.filter(|available| *available >= MIN_CONTEXT_PACK_BYTES)
				.ok_or(StoreError::InvalidInput(
					"message leaves no bounded Context Pack capacity",
				))?,
		);
		let policy = ContextPackPolicy::new(max_bytes, MAX_CONTEXT_RECENT_ITEMS)
			.map_err(|_| StoreError::InvalidInput("Context Pack policy is invalid"))?;
		let pinned = PinnedContextSource::new(
			conversation_id.as_str(),
			revision,
			format!(
				"conversation_id={}\nconversation_revision={conversation_revision}",
				conversation_id.as_str(),
			),
		)
		.map_err(|_| StoreError::InvalidInput("Conversation Context Pack pin is invalid"))?;
		compile_context_pack(ContextPackInput {
			conversation_id: conversation_id.clone(),
			possible_side_effects: PossibleSideEffects::Unknown,
			policy,
			pinned,
			optional_sources,
		})
		.map_err(|_| StoreError::InvalidInput("Context Pack compilation failed"))
	}

	async fn plan_existing_session(
		&self,
		input: ExistingSessionPlanningInput<'_>,
	) -> Result<
		(PersistedDecisionProvenance, decodex_database::ContinuationPlanEffect),
		ExistingSessionPlanningRefusal,
	> {
		let ExistingSessionPlanningInput { operation_key, consumer, message_bytes, expected } =
			input;
		let (conversation_id, turn_id) = match &consumer {
			ExecutionConsumer::ConversationTurn { conversation_id, turn_id, .. } =>
				(conversation_id.clone(), turn_id.clone()),
			ExecutionConsumer::ManagedRunExecution { .. } => {
				return Err(ExistingSessionPlanningRefusal::Conflict);
			},
		};
		let fallback_context_pack = self
			.compile_fallback_context_pack(
				&conversation_id,
				consumer.domain_revision(),
				message_bytes,
			)
			.await
			.map_err(|error| match error {
				StoreError::InvalidInput(_)
				| StoreError::Incompatible(_)
				| StoreError::CredentialRejected => ExistingSessionPlanningRefusal::Conflict,
				_ => ExistingSessionPlanningRefusal::Unknown,
			})?;
		let execution = ContinuationExecutionCommand::ordinary(
			operation_key,
			conversation_id,
			consumer.domain_revision(),
			expected.runtime_session_id.clone(),
			expected.runtime_session_revision,
			turn_id,
			fallback_context_pack,
		);
		match ExecutionCoordinator
			.continuation_bind_to_plan(&self.inner.store, &self.inner.blob_store, &execution)
			.await
		{
			PreProcessOutcome::Planned { decision, plan }
				if plan.plan.selected_account_id == *expected.account_id
					&& plan.plan.source_runtime_session_id == *expected.runtime_session_id
					&& plan.plan.source_runtime_session_revision
						== expected.runtime_session_revision
					&& match plan.plan.kind {
						decodex_core::ContinuationPlanKind::SameThread =>
							plan.plan.codex_thread_id.as_deref() == Some(expected.thread_id)
								&& plan.runtime_session.is_none()
								&& plan.fallback_context_pack.is_none(),
						decodex_core::ContinuationPlanKind::ContextPackFallback =>
							plan.runtime_session.as_ref().is_some_and(|session| {
								session.conversation_id == plan.plan.conversation_id
									&& session.state == RuntimeSessionState::Starting
									&& session.revision == 1 && session.codex_thread_id.is_none()
									&& session.account_snapshot.source_account_id
										== plan.plan.selected_account_id
							}) && plan.fallback_context_pack.is_some(),
						decodex_core::ContinuationPlanKind::InitialThread => false,
					} =>
				Ok((decision, plan)),
			PreProcessOutcome::Planned { .. } => Err(ExistingSessionPlanningRefusal::Conflict),
			PreProcessOutcome::FailedClosed(ExecutionFailureKind::ContinuationRejected(
				rejection,
			)) => match continuation_recovery(rejection) {
				Some(recovery) => Err(ExistingSessionPlanningRefusal::Recovery(recovery)),
				None => Err(ExistingSessionPlanningRefusal::Conflict),
			},
			PreProcessOutcome::FailedClosed(ExecutionFailureKind::Other) =>
				Err(ExistingSessionPlanningRefusal::Unknown),
			PreProcessOutcome::Waiting
			| PreProcessOutcome::NoRoute
			| PreProcessOutcome::EstablishmentPending => Err(ExistingSessionPlanningRefusal::Conflict),
		}
	}

	async fn resume_same_thread(
		&self,
		session: &LocalSession,
	) -> Result<FreshRuntimeSessionResume, SameThreadResumeRefusal> {
		let thread_id = ExactThreadId::new(session.codex_thread_id.clone())
			.map_err(|_| SameThreadResumeRefusal::IncompatibleThread)?;
		let request = QuickTaskThreadResumeRequest::new(
			thread_id,
			session.model.clone(),
			session.working_directory.clone(),
			session.instructions.clone(),
		)
		.map(|request| request.with_fast(session.fast))
		.map_err(|_| SameThreadResumeRefusal::IncompatibleThread)?;
		let resumed =
			self.resume_thread(&session.process, request).await.map_err(|error| match error {
				QuickTaskProcessError::Rejected { .. } => SameThreadResumeRefusal::MissingThread,
				QuickTaskProcessError::Incompatible => SameThreadResumeRefusal::IncompatibleThread,
				QuickTaskProcessError::Unavailable => SameThreadResumeRefusal::ProcessUnavailable,
				QuickTaskProcessError::ControlLost | QuickTaskProcessError::Ambiguous { .. } =>
					SameThreadResumeRefusal::Ambiguous,
			})?;
		if !resumed.events.is_empty() {
			return Err(SameThreadResumeRefusal::Ambiguous);
		}
		FreshRuntimeSessionResume::new(
			session.runtime_session_id.clone(),
			session.runtime_session_revision,
			&session.process,
			self.inner.execution_authorization.epoch_id.clone(),
			RuntimeSessionResumeRequest {
				request_id: resumed.request_id,
				request_sha256: resumed.request_sha256,
			},
			SuccessfulRuntimeSessionResume {
				response_id: resumed.response_id,
				response_sha256: resumed.response_sha256,
				codex_thread_id: resumed.codex_thread_id,
			},
		)
		.map_err(|_| SameThreadResumeRefusal::IncompatibleThread)
	}

	pub(crate) async fn submit_turn(&self, command: SubmitQuickTaskTurn) -> QuickTaskOutcome {
		if self.is_shutting_down() {
			return QuickTaskOutcome::Unavailable;
		}
		let mut session = match self.reserve_later_turn(&command) {
			Ok(session) => session,
			Err(outcome) => match *outcome {
				outcome @ QuickTaskOutcome::ManualRecovery {
					action: QuickTaskManualRecovery::MissingLocalProcess,
					..
				} => return self.submit_rehydrated_turn(command, outcome).await,
				outcome => return outcome,
			},
		};
		session.model.clone_from(&command.execution.model);
		session.reasoning_effort.clone_from(&command.execution.reasoning_effort);
		session.fast = command.execution.fast;
		let admitted = match self.admit_later_turn(&command, session).await {
			Ok(admitted) => admitted,
			Err(outcome) => return *outcome,
		};
		self.submit_admitted_turn(command, admitted).await
	}

	async fn admit_later_turn(
		&self,
		command: &SubmitQuickTaskTurn,
		mut session: LocalSession,
	) -> Result<AdmittedLaterTurn, Box<QuickTaskOutcome>> {
		let sequence = session.next_user_sequence;
		let turn_reservation = match self
			.reserve_user_turn(
				&command.operation_key,
				&command.conversation_id,
				&session.runtime_session_id,
				&command.turn_id,
				sequence,
				&command.message,
			)
			.await
		{
			Ok(reservation) => reservation,
			Err(error) if turn_reservation_is_definite(&error) => {
				self.restore_ready(session);
				return Err(Box::new(QuickTaskOutcome::Conflict));
			},
			Err(error) if turn_reservation_is_integrity_failure(&error) => {
				let outcome = self
					.recover_active_turn(
						session,
						command.turn_id.clone(),
						QuickTaskManualRecovery::PriorActiveTurn,
					)
					.await;
				return Err(Box::new(outcome));
			},
			Err(_) => {
				let outcome = self
					.ambiguous_session(
						session,
						command.turn_id.clone(),
						QuickTaskAmbiguity::TurnFinalization,
					)
					.await;
				return Err(Box::new(outcome));
			},
		};
		if !turn_admits_execution(&turn_reservation) {
			self.restore_ready(session);
			return Err(Box::new(QuickTaskOutcome::Conflict));
		}
		session.next_user_sequence = sequence.saturating_add(1);
		if !self.set_preparing(session.clone()) {
			self.terminate_process(&session.process).await;
			let outcome = self
				.finalize_bound_refusal(session, &command.turn_id, ReservedTurnRefusal::Unavailable)
				.await;
			return Err(Box::new(outcome));
		}
		let consumer = ExecutionConsumer::ConversationTurn {
			conversation_id: command.conversation_id.clone(),
			conversation_revision: session.conversation_revision,
			source_runtime_session_id: Some(session.runtime_session_id.clone()),
			source_runtime_session_revision: Some(session.runtime_session_revision),
			turn_id: command.turn_id.clone(),
		};
		Ok(AdmittedLaterTurn { session, sequence, consumer })
	}

	async fn submit_admitted_turn(
		&self,
		command: SubmitQuickTaskTurn,
		admitted: AdmittedLaterTurn,
	) -> QuickTaskOutcome {
		let AdmittedLaterTurn { session, sequence, consumer } = admitted;
		let (decision, plan) = match self
			.plan_existing_session(ExistingSessionPlanningInput {
				operation_key: &command.operation_key,
				consumer: consumer.clone(),
				message_bytes: command.message.len(),
				expected: ExistingSessionExpectation {
					account_id: &session.account_id,
					runtime_session_id: &session.runtime_session_id,
					runtime_session_revision: session.runtime_session_revision,
					thread_id: &session.codex_thread_id,
				},
			})
			.await
		{
			Ok(planned) => planned,
			Err(ExistingSessionPlanningRefusal::Recovery(recovery)) => {
				return self
					.finalize_bound_refusal(
						session,
						&command.turn_id,
						ReservedTurnRefusal::Recovery(recovery),
					)
					.await;
			},
			Err(ExistingSessionPlanningRefusal::Conflict) => {
				return self
					.finalize_bound_refusal(
						session,
						&command.turn_id,
						ReservedTurnRefusal::Conflict,
					)
					.await;
			},
			Err(ExistingSessionPlanningRefusal::Unknown) => {
				return self
					.ambiguous_session(
						session,
						command.turn_id.clone(),
						QuickTaskAmbiguity::TurnFinalization,
					)
					.await;
			},
		};
		if plan.plan.kind == decodex_core::ContinuationPlanKind::ContextPackFallback {
			return self
				.establish_context_fallback(command, sequence, decision, plan, Some(session))
				.await;
		}
		let resume = match self.resume_same_thread(&session).await {
			Ok(resume) => resume,
			Err(SameThreadResumeRefusal::MissingThread) => {
				return self
					.finalize_bound_recovery(
						session,
						&command.turn_id,
						QuickTaskManualRecovery::MissingThread,
					)
					.await;
			},
			Err(SameThreadResumeRefusal::IncompatibleThread) => {
				return self
					.finalize_bound_recovery(
						session,
						&command.turn_id,
						QuickTaskManualRecovery::IncompatibleThread,
					)
					.await;
			},
			Err(SameThreadResumeRefusal::ProcessUnavailable) => {
				return self
					.finalize_bound_recovery(
						session,
						&command.turn_id,
						QuickTaskManualRecovery::ProcessUnavailable,
					)
					.await;
			},
			Err(SameThreadResumeRefusal::Ambiguous) => {
				return self
					.ambiguous_session(
						session,
						command.turn_id.clone(),
						QuickTaskAmbiguity::ThreadResume,
					)
					.await;
			},
		};
		self.dispatch_turn(
			&command.operation_key,
			decision,
			plan,
			session,
			command.turn_id,
			sequence,
			command.message,
			None,
			ProviderAttemptRuntimeAuthority::ExistingSessionResume(resume),
		)
		.await
	}

	#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
	async fn dispatch_turn(
		&self,
		operation_key: &str,
		decision: crate::routing_orchestration::PersistedDecisionProvenance,
		plan: decodex_database::ContinuationPlanEffect,
		session: LocalSession,
		turn_id: TurnId,
		turn_sequence: i64,
		message: String,
		context_pack: Option<ContextPack>,
		runtime_authority: ProviderAttemptRuntimeAuthority,
	) -> QuickTaskOutcome {
		let thread_id = match ExactThreadId::new(session.codex_thread_id.clone()) {
			Ok(thread_id) => thread_id,
			Err(_) => {
				return self
					.finalize_bound_recovery(
						session,
						&turn_id,
						QuickTaskManualRecovery::IncompatibleThread,
					)
					.await;
			},
		};
		let input = match match context_pack {
			Some(context_pack) =>
				String::from_utf8(context_pack.bytes().to_vec()).map_err(|_| ()).and_then(
					|context| QuickTaskTurnInput::from_texts([context, message]).map_err(|_| ()),
				),
			None => QuickTaskTurnInput::text(message).map_err(|_| ()),
		} {
			Ok(input) => input,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Conflict)
					.await;
			},
		};
		let request = match QuickTaskTurnStartRequest::new(
			thread_id,
			input,
			session.model.clone(),
			session.reasoning_effort.clone(),
		)
		.map(|request| request.with_fast(session.fast))
		{
			Ok(request) => request,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Conflict)
					.await;
			},
		};
		let attempt_id = match ProviderAttemptId::new(derived_uuid(
			"provider-attempt",
			&[operation_key, turn_id.as_str()],
		)) {
			Ok(value) => value,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Unavailable)
					.await;
			},
		};
		let prepared =
			match self.prepare_turn_start(&session.process, attempt_id.clone(), request).await {
				Ok(prepared) => prepared,
				Err(_) => {
					return self
						.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Unavailable)
						.await;
				},
			};
		let request_id = match ProviderRequestId::new(derived_uuid(
			"provider-request",
			&[operation_key, turn_id.as_str()],
		)) {
			Ok(value) => value,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Unavailable)
					.await;
			},
		};
		let provider_key = match ProviderRequestKey::new(format!(
			"app-server:{}:{}",
			session.process.generation_id().as_str(),
			prepared.request_id(),
		)) {
			Ok(value) => value,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Conflict)
					.await;
			},
		};
		let provider_keys = match ProviderRequestKeys::new(None, Some(provider_key.clone())) {
			Ok(value) => value,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Conflict)
					.await;
			},
		};
		let preparation = match ProviderAttemptPreparation::new(
			attempt_id.clone(),
			ProviderAttemptConsumer::ConversationTurn {
				conversation_id: session.conversation_id.clone(),
				turn_id: turn_id.clone(),
			},
			plan.plan.plan_id.clone(),
			request_id.clone(),
			prepared.request_sha256().to_owned(),
			provider_keys,
			ProviderDuplicateRisk::OriginalIntent,
		) {
			Ok(value) => value,
			Err(_) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Conflict)
					.await;
			},
		};
		let post = ExecutionCoordinator
			.post_process(
				&self.inner.provider_attempts,
				&session.process,
				PostProcessCommand {
					decision,
					plan,
					provider_attempt: preparation,
					runtime_authority,
				},
			)
			.await;
		let fresh = match post {
			PostProcessOutcome::FreshPrepared { attempt, fresh_preparation: fresh, .. }
				if attempt.newly_prepared
					&& attempt.attempt_id == attempt_id
					&& attempt.revision == fresh.revision() =>
				fresh,
			PostProcessOutcome::PreparedReplay { attempt, .. }
				if !attempt.newly_prepared
					&& attempt.attempt_id == attempt_id
					&& attempt.revision > 0 =>
			{
				return match self.cancel_prepared_exact(&attempt_id, attempt.revision).await {
					PreparedCancellationDisposition::Canceled =>
						self.finalize_bound_refusal(
							session,
							&turn_id,
							ReservedTurnRefusal::Conflict,
						)
						.await,
					PreparedCancellationDisposition::Conflict =>
						self.recover_active_turn(
							session,
							turn_id,
							QuickTaskManualRecovery::PriorAttemptUnresolved,
						)
						.await,
					PreparedCancellationDisposition::Ambiguous =>
						self.ambiguous_session(session, turn_id, QuickTaskAmbiguity::TurnStart)
							.await,
				};
			},
			PostProcessOutcome::DefiniteRejection(DefinitePostProcessRefusal::NoAttempt) => {
				return self
					.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Conflict)
					.await;
			},
			PostProcessOutcome::DefiniteRejection(DefinitePostProcessRefusal::ExistingAttempt)
			| PostProcessOutcome::FreshPrepared { .. }
			| PostProcessOutcome::PreparedReplay { .. } => {
				return self
					.recover_active_turn(
						session,
						turn_id,
						QuickTaskManualRecovery::PriorAttemptUnresolved,
					)
					.await;
			},
			PostProcessOutcome::EffectOrPersistenceAmbiguity => {
				return self
					.ambiguous_session(session, turn_id, QuickTaskAmbiguity::TurnStart)
					.await;
			},
		};
		let prepared_revision = fresh.revision();
		if self.is_shutting_down() {
			return match self.cancel_prepared_exact(&attempt_id, prepared_revision).await {
				PreparedCancellationDisposition::Canceled =>
					self.finalize_bound_refusal(session, &turn_id, ReservedTurnRefusal::Unavailable)
						.await,
				PreparedCancellationDisposition::Conflict =>
					self.recover_active_turn(
						session,
						turn_id,
						QuickTaskManualRecovery::PriorAttemptUnresolved,
					)
					.await,
				PreparedCancellationDisposition::Ambiguous =>
					self.ambiguous_session(session, turn_id, QuickTaskAmbiguity::TurnStart).await,
			};
		}
		let authorization =
			match self.inner.provider_attempts.authorize_dispatch(fresh, &session.process).await {
				Ok(AuthorizeProviderDispatchOutcome::Fresh(fence)) => fence,
				Ok(
					AuthorizeProviderDispatchOutcome::Replayed(actual)
					| AuthorizeProviderDispatchOutcome::Rejected { actual, .. },
				) if matches!(
					actual.state,
					ProviderAttemptState::Prepared | ProviderAttemptState::Canceled
				) =>
				{
					return match self.cancel_prepared_exact(&attempt_id, prepared_revision).await {
						PreparedCancellationDisposition::Canceled =>
							self.finalize_bound_refusal(
								session,
								&turn_id,
								ReservedTurnRefusal::Conflict,
							)
							.await,
						PreparedCancellationDisposition::Conflict =>
							self.recover_active_turn(
								session,
								turn_id,
								QuickTaskManualRecovery::PriorAttemptUnresolved,
							)
							.await,
						PreparedCancellationDisposition::Ambiguous =>
							self.ambiguous_session(session, turn_id, QuickTaskAmbiguity::TurnStart)
								.await,
					};
				},
				Ok(
					AuthorizeProviderDispatchOutcome::Replayed(actual)
					| AuthorizeProviderDispatchOutcome::Rejected { actual, .. },
				) => {
					if actual.state == ProviderAttemptState::DispatchAuthorized {
						let _ = self
							.inner
							.provider_attempts
							.mark_unknown(&attempt_id, actual.revision)
							.await;
					}
					return self
						.ambiguous_session(session, turn_id, QuickTaskAmbiguity::TurnStart)
						.await;
				},
				Err(_) => {
					return self
						.ambiguous_session(session, turn_id, QuickTaskAmbiguity::TurnStart)
						.await;
				},
			};
		let authorized_revision = authorization.attempt_revision();
		if authorization.attempt_id() != &attempt_id
			|| authorization.process_generation_id() != session.process.generation_id()
			|| authorization.process_generation_revision() != session.process.revision()
		{
			let _ =
				self.inner.provider_attempts.mark_unknown(&attempt_id, authorized_revision).await;
			return self
				.ambiguous_session(session, turn_id.clone(), QuickTaskAmbiguity::TurnStart)
				.await;
		}
		let started = match self.start_turn(&session.process, prepared, authorization).await {
			Ok(started) => started,
			Err(_) => {
				let _ = self
					.inner
					.provider_attempts
					.mark_unknown(&attempt_id, authorized_revision)
					.await;
				return self
					.ambiguous_session(session, turn_id.clone(), QuickTaskAmbiguity::TurnStart)
					.await;
			},
		};
		let (commands, command_receiver) = mpsc::sync_channel(PROCESS_COMMAND_CAPACITY);
		let context = TurnContext {
			session: session.clone(),
			logical_turn_id: turn_id.clone(),
			logical_turn_sequence: turn_sequence,
			assistant_turn_id: TurnId::new(derived_uuid(
				"assistant-turn",
				&[operation_key, turn_id.as_str()],
			))
			.expect("derived UUID-v4 is valid"),
			attempt_id,
			request_id,
			provider_key,
			provider_turn_id: started.turn_id.clone(),
			authorized_revision,
		};
		let mut workers = self.inner.workers.lock().await;
		while workers.try_join_next().is_some() {}
		if self.is_shutting_down()
			|| !self.set_active(session.clone(), turn_id, context.attempt_id.clone(), commands)
			|| self.is_shutting_down()
		{
			drop(workers);
			let _ = self
				.inner
				.provider_attempts
				.mark_unknown(&context.attempt_id, context.authorized_revision)
				.await;
			return self
				.ambiguous_session(
					session,
					context.logical_turn_id.clone(),
					QuickTaskAmbiguity::ActiveTurn,
				)
				.await;
		}
		let readback = session_readback(
			&session,
			QuickTaskLocalState::Running,
			Some(context.logical_turn_id.clone()),
		);
		let outcome =
			QuickTaskOutcome::Started { readback, provider_turn_id: started.turn_id.clone() };
		let runtime = self.clone();
		workers.spawn(async move {
			runtime.drive_turn(context, started, command_receiver).await;
		});
		drop(workers);
		outcome
	}

	async fn drive_turn(
		&self,
		context: TurnContext,
		started: StartedOrdinaryTurn,
		commands: mpsc::Receiver<WorkerCommand>,
	) {
		let mut assistant_ordinal = 0_i32;
		for event in started.events {
			match self.handle_process_event(&context, &mut assistant_ordinal, event).await {
				Ok(true) => return,
				Ok(false) => {},
				Err(()) => {
					self.mark_active_unknown(&context, QuickTaskAmbiguity::ActiveTurn).await;
					return;
				},
			}
		}
		if started.status != QuickTaskTurnStatus::InProgress {
			let status = match started.status {
				QuickTaskTurnStatus::Completed => TurnStatus::Completed,
				QuickTaskTurnStatus::Interrupted => TurnStatus::Interrupted,
				QuickTaskTurnStatus::Failed => TurnStatus::Failed,
				QuickTaskTurnStatus::InProgress => unreachable!(),
			};
			let handled = self
				.handle_process_event(
					&context,
					&mut assistant_ordinal,
					QuickTaskProcessEvent::TurnCompleted {
						turn_id: context.provider_turn_id.clone(),
						status,
						witness_digest: started.response_sha256,
					},
				)
				.await;
			if handled != Ok(true) {
				self.mark_active_unknown(&context, QuickTaskAmbiguity::ActiveTurn).await;
			}
			return;
		}
		let (output, mut outputs) = tokio_mpsc::channel(EVENT_QUEUE_CAPACITY);
		let control = self.inner.process_generations.clone();
		let process = context.session.process.clone();
		let thread_id = context.session.codex_thread_id.clone();
		let provider_turn_id = context.provider_turn_id.clone();
		let shutting_down = Arc::clone(&self.inner.shutting_down);
		let worker = task::spawn_blocking(move || {
			run_event_worker(
				control,
				process,
				thread_id,
				provider_turn_id,
				commands,
				shutting_down,
				output,
			)
		});
		let mut terminal = false;
		let mut stop_worker = false;
		while let Some(output) = outputs.recv().await {
			match output {
				WorkerOutput::Event(event) => {
					match self.handle_process_event(&context, &mut assistant_ordinal, event).await {
						Ok(is_terminal) => {
							terminal = is_terminal;
							if terminal {
								break;
							}
						},
						Err(()) => {
							stop_worker = true;
							break;
						},
					}
				},
				WorkerOutput::Failed => break,
			}
		}
		if stop_worker {
			self.stop_active_worker(&context.attempt_id, &context.session.conversation_id);
		}
		drop(outputs);
		let _ = worker.await;
		if !terminal {
			self.mark_active_unknown(&context, QuickTaskAmbiguity::ActiveTurn).await;
		}
	}

	async fn handle_process_event(
		&self,
		context: &TurnContext,
		assistant_ordinal: &mut i32,
		event: QuickTaskProcessEvent,
	) -> Result<bool, ()> {
		match event {
			QuickTaskProcessEvent::MessageDelta(delta) => {
				if delta.thread_id().as_str() != context.session.codex_thread_id.as_str()
					|| delta.turn_id().as_str() != context.provider_turn_id.as_str()
				{
					return Err(());
				}
				let assistant_sequence = context.logical_turn_sequence.checked_add(1).ok_or(())?;
				let assistant_sequence_text = assistant_sequence.to_string();
				let text = delta.text();
				let mut offset = 0;
				let mut first = true;
				while first || offset < text.len() {
					first = false;
					let end = history_chunk_end(text, offset);
					let bounded_text =
						HistoryText::new(text[offset..end].to_owned()).map_err(|_| ())?;
					let assistant_ordinal_text = assistant_ordinal.to_string();
					let history_item_id = HistoryItemId::new(derived_uuid(
						"assistant-history",
						&[context.attempt_id.as_str(), &assistant_ordinal_text],
					))
					.map_err(|_| ())?;
					let command = exact_command(
						"assistant-history",
						history_item_id.as_str(),
						&[
							context.attempt_id.as_str(),
							context.session.conversation_id.as_str(),
							context.session.runtime_session_id.as_str(),
							context.assistant_turn_id.as_str(),
							&assistant_sequence_text,
							history_item_id.as_str(),
							&assistant_ordinal_text,
							bounded_text.as_str(),
						],
					)
					.map_err(|_| ())?;
					self.inner
						.store
						.record_history_item(
							&self.inner.blob_store,
							&command,
							&RecordHistoryItem {
								conversation_id: context.session.conversation_id.clone(),
								runtime_session_id: context.session.runtime_session_id.clone(),
								turn_id: context.assistant_turn_id.clone(),
								turn_sequence: assistant_sequence,
								turn_role: TurnRole::Assistant,
								possible_side_effects: PossibleSideEffects::Unknown,
								history_item_id: history_item_id.clone(),
								ordinal: *assistant_ordinal,
								kind: HistoryItemKind::Message,
								status: ItemStatus::Completed,
								text: bounded_text.as_str().to_owned(),
								media_type: markdown_media_type(),
								metadata: HistoryMetadata::empty(),
								expected_revision: None,
								artifact: None,
							},
						)
						.await
						.map_err(|_| ())?;
					*assistant_ordinal = (*assistant_ordinal).checked_add(1).ok_or(())?;
					let readback = session_readback(
						&context.session,
						QuickTaskLocalState::Running,
						Some(context.logical_turn_id.clone()),
					);
					self.emit(QuickTaskOutcome::Streaming {
						readback,
						history_item_id,
						text: bounded_text,
					})
					.await;
					offset = end;
				}
				Ok(false)
			},
			QuickTaskProcessEvent::TurnCompleted { turn_id, status, witness_digest } => {
				if turn_id != context.provider_turn_id {
					return Err(());
				}
				self.finish_positive_turn(context, status, witness_digest, *assistant_ordinal > 0)
					.await?;
				Ok(true)
			},
		}
	}

	async fn finish_positive_turn(
		&self,
		context: &TurnContext,
		status: TurnStatus,
		witness_digest: String,
		has_assistant_turn: bool,
	) -> Result<(), ()> {
		let (terminal, outcome) = match status {
			TurnStatus::Completed =>
				(QuickTaskTerminalState::Succeeded, ProviderTerminalOutcome::Succeeded),
			TurnStatus::Interrupted | TurnStatus::Failed =>
				(QuickTaskTerminalState::Failed, ProviderTerminalOutcome::FailedDefinitive),
			TurnStatus::InProgress | TurnStatus::Unknown => return Err(()),
		};
		let evidence_id = ProviderEvidenceId::new(derived_uuid(
			"provider-evidence",
			&[context.attempt_id.as_str()],
		))
		.map_err(|_| ())?;
		let evidence = ProviderPositiveEvidence::new(
			evidence_id.clone(),
			context.attempt_id.clone(),
			context.request_id.clone(),
			ProviderEvidenceSource::ProviderReceipt,
			outcome,
			context.provider_key.clone(),
			Some(format!("app-server-turn:{}", request_digest(&[&context.provider_turn_id]))),
			Some(context.session.codex_thread_id.clone()),
			Some(context.provider_turn_id.clone()),
			witness_digest,
		)
		.map_err(|_| ())?;
		match self
			.inner
			.provider_attempts
			.record_positive_evidence(&evidence)
			.await
			.map_err(|_| ())?
		{
			ProviderAttemptReconciliation::PositiveEvidenceRecorded { state }
			| ProviderAttemptReconciliation::AlreadyTerminal { state }
				if state == outcome.state() => {},
			_ => return Err(()),
		}
		let terminal_attempt_revision = context.authorized_revision.checked_add(1).ok_or(())?;
		let terminalization = TerminalizeQuickTaskTurn {
			conversation_id: context.session.conversation_id.clone(),
			expected_conversation_revision: context.session.conversation_revision,
			runtime_session_id: context.session.runtime_session_id.clone(),
			expected_runtime_session_revision: context.session.runtime_session_revision,
			user_turn_id: context.logical_turn_id.clone(),
			expected_user_turn_revision: 1,
			assistant_turn: has_assistant_turn.then(|| (context.assistant_turn_id.clone(), 1)),
			provider_attempt_id: context.attempt_id.clone(),
			expected_provider_attempt_revision: terminal_attempt_revision,
			provider_evidence_id: evidence_id,
			provider_outcome: outcome,
			provider_thread_id: context.session.codex_thread_id.clone(),
			provider_turn_id: context.provider_turn_id.clone(),
		};
		let terminalization = match self
			.inner
			.store
			.terminalize_quick_task_turn(
				&scoped_key("turn-terminalization", context.attempt_id.as_str()),
				&terminalization,
			)
			.await
			.map_err(|_| ())?
		{
			QuickTaskTerminalizationOutcome::Applied(readback)
			| QuickTaskTerminalizationOutcome::Replayed(readback) => readback,
			QuickTaskTerminalizationOutcome::Rejected
			| QuickTaskTerminalizationOutcome::Unknown => return Err(()),
		};
		let mut session = context.session.clone();
		session.runtime_session_revision = terminalization.runtime_session_revision;
		session.has_acknowledged_turn = true;
		let sequence_increment = if has_assistant_turn { 2 } else { 1 };
		session.next_user_sequence =
			context.logical_turn_sequence.saturating_add(sequence_increment);
		let readback = session_readback(&session, QuickTaskLocalState::Ready, None);
		if self.retire_process(&session.process).await {
			if !self.remove_active_if(&context.attempt_id, &session.conversation_id) {
				return Err(());
			}
			self.emit(QuickTaskOutcome::Terminal {
				readback,
				turn_id: context.logical_turn_id.clone(),
				state: terminal,
				provider_turn_id: context.provider_turn_id.clone(),
			})
			.await;
		} else {
			let mut recovery = readback;
			recovery.state = QuickTaskLocalState::ManualRecovery;
			if !self.set_recovery_if_active(
				&context.attempt_id,
				recovery.clone(),
				QuickTaskManualRecovery::ProcessUnavailable,
			) {
				return Err(());
			}
			self.emit(QuickTaskOutcome::ManualRecovery {
				readback: recovery,
				action: QuickTaskManualRecovery::ProcessUnavailable,
			})
			.await;
		}
		Ok(())
	}

	pub(crate) fn interrupt(&self, conversation_id: &ConversationId) -> QuickTaskOutcome {
		let mut local = self.local();
		let Some(task) = local.get_mut(conversation_id.as_str()) else {
			return QuickTaskOutcome::ManualRecovery {
				readback: empty_readback(conversation_id, QuickTaskLocalState::ManualRecovery),
				action: QuickTaskManualRecovery::MissingLocalProcess,
			};
		};
		match &task.state {
			LocalTaskState::Active { session, turn_id, commands, .. } => {
				let readback =
					session_readback(session, QuickTaskLocalState::Running, Some(turn_id.clone()));
				match commands.try_send(WorkerCommand::Interrupt) {
					Ok(()) => QuickTaskOutcome::InterruptRequested(readback),
					Err(mpsc::TrySendError::Full(_)) => QuickTaskOutcome::Busy(readback),
					Err(mpsc::TrySendError::Disconnected(_)) => QuickTaskOutcome::Unknown {
						readback,
						ambiguity: QuickTaskAmbiguity::ActiveTurn,
					},
				}
			},
			_ => QuickTaskOutcome::Conflict,
		}
	}

	pub(crate) fn projection(
		&self,
		conversation_id: &ConversationId,
	) -> Option<QuickTaskProjection> {
		let local = self.local();
		let task = local.get(conversation_id.as_str())?;
		if matches!(&task.state, LocalTaskState::Establishing) {
			return None;
		}
		let readback = local_readback(conversation_id, task);
		let recovery = match &task.state {
			LocalTaskState::Recovery { action, .. }
				if readback.state == QuickTaskLocalState::ManualRecovery =>
				Some(*action),
			_ => None,
		};
		Some(QuickTaskProjection { readback, recovery })
	}

	/// Refresh or archive one exact selected Codex thread without dispatching a model turn.
	pub(crate) async fn control_thread(
		&self,
		command: ControlQuickTask,
	) -> QuickTaskControlOutcome {
		if self.is_shutting_down()
			|| command.expected_conversation_revision <= 0
			|| command.expected_runtime_session_revision <= 0
		{
			return QuickTaskControlOutcome::Unavailable;
		}
		{
			let local = self.local();
			if let Some(task) = local.get(command.conversation_id.as_str()) {
				match &task.state {
					LocalTaskState::Active { .. }
					| LocalTaskState::Preparing(_)
					| LocalTaskState::Establishing => return QuickTaskControlOutcome::Busy,
					LocalTaskState::Recovery { readback, .. }
						if readback.state == QuickTaskLocalState::OutcomeUnknown =>
					{
						return QuickTaskControlOutcome::Conflict;
					},
					LocalTaskState::Recovery { .. } => {},
					LocalTaskState::Ready(_) => {},
				}
			}
		}
		let session = match self
			.inner
			.store
			.read_ordinary_runtime_session_for_resume(&command.conversation_id)
			.await
		{
			Ok(Some(session)) => session,
			Ok(None) => return self.archive_local_control_thread(&command).await,
			Err(_) => return QuickTaskControlOutcome::Unavailable,
		};
		if session.conversation_revision != command.expected_conversation_revision
			|| session.runtime_session_id != command.runtime_session_id
			|| session.runtime_session_revision != command.expected_runtime_session_revision
			|| session.has_unresolved_provider_attempt
			|| session.has_unresolved_process_generation
			|| session.active_turn_id != command.active_turn_id
			|| session.active_turn_revision != command.active_turn_revision
		{
			return QuickTaskControlOutcome::Conflict;
		}
		if let (Some(turn_id), Some(turn_revision)) =
			(session.active_turn_id.as_ref(), session.active_turn_revision)
			&& let Err(outcome) =
				self.reconcile_control_turn(&command, turn_id, turn_revision).await
		{
			return outcome;
		}
		let request = match self.inner.store.read_quick_task_request(&command.conversation_id).await {
			Ok(Some(request)) => request,
			Ok(None) => return QuickTaskControlOutcome::Conflict,
			Err(_) => return QuickTaskControlOutcome::Unavailable,
		};
		let archived = match self
			.observe_control_thread(
				&command.operation_key,
				&session.source_account_id,
				session.source_account_revision,
				&request.working_directory,
				&session.codex_thread_id,
				command.archive,
			)
			.await
		{
			Ok(archived) => archived,
			Err(outcome) => return outcome,
		};
		if !archived {
			return QuickTaskControlOutcome::Current;
		}
		let persistence = match exact_command(
			"archive-conversation",
			&command.operation_key,
			&[
				command.conversation_id.as_str(),
				&session.conversation_revision.to_string(),
				session.runtime_session_id.as_str(),
				&session.runtime_session_revision.to_string(),
				session.codex_thread_id.as_str(),
			],
		) {
			Ok(command) => command,
			Err(()) => return QuickTaskControlOutcome::Conflict,
		};
		let archived = match self
			.inner
			.store
			.archive_quick_task_conversation(
				&persistence,
				&ArchiveQuickTaskConversation {
					conversation_id: command.conversation_id.clone(),
					expected_conversation_revision: session.conversation_revision,
					runtime_session_id: session.runtime_session_id,
					expected_runtime_session_revision: session.runtime_session_revision,
				},
			)
			.await
		{
			Ok(ArchiveQuickTaskConversationOutcome::Applied(archived))
			| Ok(ArchiveQuickTaskConversationOutcome::Replayed(archived)) => archived,
			Ok(ArchiveQuickTaskConversationOutcome::Rejected) => {
				return QuickTaskControlOutcome::Conflict;
			},
			Err(_) => return QuickTaskControlOutcome::OutcomeUnknown,
		};
		let process = {
			let mut local = self.local();
			local.remove(command.conversation_id.as_str()).and_then(|task| match task.state {
				LocalTaskState::Ready(session) => Some(session.process),
				_ => None,
			})
		};
		if let Some(process) = process {
			self.terminate_process(&process).await;
		}
		QuickTaskControlOutcome::Archived {
			conversation_revision: archived.conversation_revision,
		}
	}

	async fn reconcile_control_turn(
		&self,
		command: &ControlQuickTask,
		turn_id: &TurnId,
		expected_turn_revision: i64,
	) -> Result<(), QuickTaskControlOutcome> {
		let persistence = exact_command(
			"reconcile-stranded-turn",
			&command.operation_key,
			&[
				command.conversation_id.as_str(),
				&command.expected_conversation_revision.to_string(),
				command.runtime_session_id.as_str(),
				&command.expected_runtime_session_revision.to_string(),
				turn_id.as_str(),
				&expected_turn_revision.to_string(),
			],
		)
		.map_err(|_| QuickTaskControlOutcome::Conflict)?;
		let outcome = self
			.inner
			.store
			.reconcile_stranded_quick_task_turn(
				&persistence,
				&ReconcileStrandedQuickTaskTurn {
					conversation_id: command.conversation_id.clone(),
					expected_conversation_revision: command.expected_conversation_revision,
					runtime_session_id: command.runtime_session_id.clone(),
					expected_runtime_session_revision: command.expected_runtime_session_revision,
					turn_id: turn_id.clone(),
					expected_turn_revision,
				},
			)
			.await
			.map_err(|_| QuickTaskControlOutcome::OutcomeUnknown)?;
		match outcome {
			ReconcileStrandedQuickTaskTurnOutcome::Applied { turn_revision }
			| ReconcileStrandedQuickTaskTurnOutcome::Replayed { turn_revision }
				if expected_turn_revision.checked_add(1) == Some(turn_revision) => {},
			ReconcileStrandedQuickTaskTurnOutcome::Applied { .. }
			| ReconcileStrandedQuickTaskTurnOutcome::Replayed { .. }
			| ReconcileStrandedQuickTaskTurnOutcome::Rejected => {
				return Err(QuickTaskControlOutcome::Conflict);
			},
		}
		let mut local = self.local();
		if local
			.get(command.conversation_id.as_str())
			.is_some_and(|task| matches!(&task.state, LocalTaskState::Recovery { .. }))
		{
			local.remove(command.conversation_id.as_str());
		}
		Ok(())
	}

	async fn archive_local_control_thread(
		&self,
		command: &ControlQuickTask,
	) -> QuickTaskControlOutcome {
		match (command.active_turn_id.as_ref(), command.active_turn_revision) {
			(Some(turn_id), Some(turn_revision)) => {
				if let Err(outcome) =
					self.reconcile_control_turn(command, turn_id, turn_revision).await
				{
					return outcome;
				}
			},
			(None, None) => {},
			_ => return QuickTaskControlOutcome::Conflict,
		}
		let persistence = match exact_command(
			"archive-local-conversation",
			&command.operation_key,
			&[
				command.conversation_id.as_str(),
				&command.expected_conversation_revision.to_string(),
				command.runtime_session_id.as_str(),
				&command.expected_runtime_session_revision.to_string(),
			],
		) {
			Ok(command) => command,
			Err(()) => return QuickTaskControlOutcome::Conflict,
		};
		let archived = match self
			.inner
			.store
			.archive_local_quick_task_conversation(
				&persistence,
				&ArchiveLocalQuickTaskConversation {
					conversation_id: command.conversation_id.clone(),
					expected_conversation_revision: command.expected_conversation_revision,
					runtime_session_id: command.runtime_session_id.clone(),
					expected_runtime_session_revision: command.expected_runtime_session_revision,
				},
			)
			.await
		{
			Ok(ArchiveLocalQuickTaskConversationOutcome::Applied(archived))
			| Ok(ArchiveLocalQuickTaskConversationOutcome::Replayed(archived)) => archived,
			Ok(ArchiveLocalQuickTaskConversationOutcome::Rejected) => {
				return QuickTaskControlOutcome::Conflict;
			},
			Err(_) => return QuickTaskControlOutcome::OutcomeUnknown,
		};
		self.local().remove(command.conversation_id.as_str());
		QuickTaskControlOutcome::Archived { conversation_revision: archived.conversation_revision }
	}

	async fn observe_control_thread(
		&self,
		operation_key: &str,
		account_id: &AccountId,
		account_revision: i64,
		working_directory: &str,
		thread_id: &str,
		archive: bool,
	) -> Result<bool, QuickTaskControlOutcome> {
		let credential = self
			.inner
			.accounts
			.process_credential(account_id, account_revision)
			.await
			.map_err(|_| QuickTaskControlOutcome::Unavailable)?;
		let profile = self.inner.launch_profile.clone();
		let capacity = Arc::clone(&self.inner.capacity);
		let accounts = Arc::clone(&self.inner.accounts);
		let runtime = tokio::runtime::Handle::current();
		let account_id = account_id.clone();
		let working_directory = working_directory.to_owned();
		let thread_id = thread_id.to_owned();
		let generation_id = ProcessGenerationId::new(derived_uuid(
			"thread-control-process",
			&[operation_key, thread_id.as_str()],
		))
		.map_err(|_| QuickTaskControlOutcome::Conflict)?;
		task::spawn_blocking(move || {
			let selected = Arc::new(
				SelectedWorkingDirectory::acquire(&working_directory)
					.map_err(|()| QuickTaskControlOutcome::Unavailable)?,
			);
			let callback: Arc<dyn ProcessAccountRefreshCallback> =
				Arc::new(QuickTaskRefreshCallback {
					accounts,
					runtime,
					generation_id,
				});
			let binding = AccountBinding::shared_home_bound(
				account_id.clone(),
				credential.binding,
				callback,
			)
			.map_err(|_| QuickTaskControlOutcome::Unavailable)?;
			let vault = QuickTaskCredentialVault {
				account_id: account_id.clone(),
				stored: credential.stored,
			};
			let permit = capacity
				.reserve(account_id, account_revision)
				.map_err(|_| QuickTaskControlOutcome::Unavailable)?;
			let launch = AttestedAppServerLaunch::bind_selected_control_working_directory(
				profile,
				working_directory.into(),
				binding,
				PROCESS_TIMEOUT,
				permit,
				selected,
			)
			.map_err(|_| QuickTaskControlOutcome::Unavailable)?;
			let mut child = launch.spawn().map_err(|_| QuickTaskControlOutcome::Unavailable)?;
			if child.initialize_ordinary_turns(&vault).is_err() {
				let _ = child.shutdown();
				return Err(QuickTaskControlOutcome::Unavailable);
			}
			drop(credential.launch_guard);
			let thread_id = ExactThreadId::new(thread_id)
				.map_err(|_| QuickTaskControlOutcome::Conflict)?;
			let observation = if archive {
				match child
					.archive_exact_ordinary_thread(&thread_id)
					.map_err(|_| QuickTaskControlOutcome::OutcomeUnknown)
				{
					Ok(ArchiveReconciliationOutcome::Archived)
					| Ok(ArchiveReconciliationOutcome::AlreadyArchived) => Ok(true),
					Ok(ArchiveReconciliationOutcome::Unverified(_)) | Err(_) => {
						Err(QuickTaskControlOutcome::OutcomeUnknown)
					},
				}
			} else {
				child
					.read_exact_ordinary_thread(&thread_id)
					.map(|readback| readback.facts.archived)
					.map_err(|_| QuickTaskControlOutcome::Unavailable)
			};
			child.shutdown().map_err(|_| QuickTaskControlOutcome::OutcomeUnknown)?;
			observation
		})
		.await
		.map_err(|_| QuickTaskControlOutcome::Unavailable)?
	}

	pub(crate) async fn next_event(&self) -> Option<QuickTaskOutcome> {
		let mut closed = self.inner.event_stream_closed.subscribe();
		let mut receiver = self.inner.event_receiver.lock().await;
		if *closed.borrow() {
			return receiver.try_recv().ok();
		}
		tokio::select! {
			biased;

			event = receiver.recv() => event,
			changed = closed.changed() => {
				if changed.is_err() || *closed.borrow() {
					receiver.try_recv().ok()
				} else {
					None
				}
			},
		}
	}

	pub(crate) fn begin_shutdown(&self) {
		self.inner.shutting_down.store(true, std::sync::atomic::Ordering::Release);
		let local = self.local();
		for task in local.values() {
			if let LocalTaskState::Active { commands, .. } = &task.state {
				let _ = commands.try_send(WorkerCommand::Shutdown);
			}
		}
	}

	pub(crate) async fn wait_for_shutdown(&self) {
		let mut workers = {
			let mut shared = self.inner.workers.lock().await;
			std::mem::take(&mut *shared)
		};
		while workers.join_next().await.is_some() {}
		let processes = {
			let mut local = self.local();
			let processes = local
				.values()
				.filter_map(|task| match &task.state {
					LocalTaskState::Preparing(session)
					| LocalTaskState::Ready(session)
					| LocalTaskState::Active { session, .. } => Some(session.process.clone()),
					LocalTaskState::Establishing | LocalTaskState::Recovery { .. } => None,
				})
				.collect::<Vec<_>>();
			local.clear();
			processes
		};
		for process in processes {
			self.terminate_process(&process).await;
		}
		self.inner.event_stream_closed.send_replace(true);
	}

	async fn reserve_user_turn(
		&self,
		operation_key: &str,
		conversation_id: &ConversationId,
		runtime_session_id: &RuntimeSessionId,
		turn_id: &TurnId,
		sequence: i64,
		message: &str,
	) -> Result<TurnReservationOutcome, StoreError> {
		let history_item_id =
			HistoryItemId::new(derived_uuid("user-history", &[operation_key, turn_id.as_str()]))
				.map_err(|_| StoreError::InvalidInput("user Turn history identity is invalid"))?;
		let command = exact_command(
			"user-history",
			operation_key,
			&[
				conversation_id.as_str(),
				runtime_session_id.as_str(),
				turn_id.as_str(),
				history_item_id.as_str(),
				message,
			],
		)
		.map_err(|_| StoreError::InvalidInput("user Turn command identity is invalid"))?;
		self.inner
			.store
			.reserve_user_turn_with_history_item(
				&self.inner.blob_store,
				&command,
				&RecordHistoryItem {
					conversation_id: conversation_id.clone(),
					runtime_session_id: runtime_session_id.clone(),
					turn_id: turn_id.clone(),
					turn_sequence: sequence,
					turn_role: TurnRole::User,
					possible_side_effects: PossibleSideEffects::Unknown,
					history_item_id,
					ordinal: 0,
					kind: HistoryItemKind::Message,
					status: ItemStatus::Completed,
					text: message.to_owned(),
					media_type: markdown_media_type(),
					metadata: HistoryMetadata::empty(),
					expected_revision: None,
					artifact: None,
				},
			)
			.await
	}

	async fn prepare_thread_start(
		&self,
		process: &FencedProcess,
		request: QuickTaskThreadStartRequest,
	) -> Result<PreparedThreadStart, QuickTaskProcessError> {
		let control = self.inner.process_generations.clone();
		let process = process.clone();
		match task::spawn_blocking(move || {
			control
				.with_fenced_child(&process, |child| child.prepare_ordinary_thread_start(&request))
		})
		.await
		{
			Ok(Ok(result)) => result,
			Ok(Err(_)) | Err(_) => Err(QuickTaskProcessError::Unavailable),
		}
	}

	async fn start_thread(
		&self,
		process: &FencedProcess,
		prepared: PreparedThreadStart,
		authority: decodex_database::FreshRuntimeSessionThreadStart,
	) -> Result<EstablishedOrdinaryThread, QuickTaskProcessError> {
		let fence = authority.readback();
		if &fence.process_generation_id != process.generation_id()
			|| fence.process_generation_revision != process.revision()
			|| fence.process_execution_epoch_id != self.inner.execution_authorization.epoch_id
		{
			return Err(QuickTaskProcessError::Incompatible);
		}
		let control = self.inner.process_generations.clone();
		let process = process.clone();
		match task::spawn_blocking(move || {
			control.with_fenced_child(&process, |child| {
				child.start_ordinary_thread(prepared, authority)
			})
		})
		.await
		{
			Ok(Ok(result)) => result,
			Ok(Err(_)) => Err(QuickTaskProcessError::Unavailable),
			Err(_) => Err(QuickTaskProcessError::ControlLost),
		}
	}

	async fn resume_thread(
		&self,
		process: &FencedProcess,
		request: QuickTaskThreadResumeRequest,
	) -> Result<ResumedOrdinaryThread, QuickTaskProcessError> {
		let control = self.inner.process_generations.clone();
		let process = process.clone();
		match task::spawn_blocking(move || {
			control.with_fenced_child(&process, |child| child.resume_ordinary_thread(&request))
		})
		.await
		{
			Ok(Ok(result)) => result,
			Ok(Err(_)) => Err(QuickTaskProcessError::Unavailable),
			Err(_) => Err(QuickTaskProcessError::ControlLost),
		}
	}

	async fn prepare_turn_start(
		&self,
		process: &FencedProcess,
		attempt_id: ProviderAttemptId,
		request: QuickTaskTurnStartRequest,
	) -> Result<PreparedTurnStart, QuickTaskProcessError> {
		let control = self.inner.process_generations.clone();
		let process = process.clone();
		match task::spawn_blocking(move || {
			control.with_fenced_child(&process, |child| {
				child.prepare_ordinary_turn_start(attempt_id, &request)
			})
		})
		.await
		{
			Ok(Ok(result)) => result,
			Ok(Err(_)) | Err(_) => Err(QuickTaskProcessError::Unavailable),
		}
	}

	async fn start_turn(
		&self,
		process: &FencedProcess,
		prepared: PreparedTurnStart,
		authority: decodex_database::FreshProviderDispatchFence,
	) -> Result<StartedOrdinaryTurn, QuickTaskProcessError> {
		let control = self.inner.process_generations.clone();
		let process = process.clone();
		match task::spawn_blocking(move || {
			control
				.with_fenced_child(&process, |child| child.start_ordinary_turn(prepared, authority))
		})
		.await
		{
			Ok(Ok(result)) => result,
			Ok(Err(_)) => Err(QuickTaskProcessError::Unavailable),
			Err(_) => Err(QuickTaskProcessError::ControlLost),
		}
	}

	async fn launch_process(
		&self,
		account_id: &AccountId,
		account_revision: i64,
		admission: FreshQuickTaskProcessGeneration,
		working_directory: &str,
	) -> Result<FencedProcess, QuickTaskManualRecovery> {
		let credential = self
			.inner
			.accounts
			.process_credential(account_id, account_revision)
			.await
			.map_err(account_recovery)?;
		self.launch_process_with_credential(account_id, admission, credential, working_directory)
			.await
	}

	async fn launch_process_with_credential(
		&self,
		account_id: &AccountId,
		admission: FreshQuickTaskProcessGeneration,
		credential: AccountProcessCredential,
		working_directory: &str,
	) -> Result<FencedProcess, QuickTaskManualRecovery> {
		let generation_id = admission.readback().request.process_generation_id.clone();
		let callback: Arc<dyn ProcessAccountRefreshCallback> = Arc::new(QuickTaskRefreshCallback {
			accounts: Arc::clone(&self.inner.accounts),
			runtime: tokio::runtime::Handle::current(),
			generation_id: generation_id.clone(),
		});
		let profile = self.inner.launch_profile.clone();
		let capacity = Arc::clone(&self.inner.capacity);
		let working_directory = working_directory.to_owned();
		let account_id_for_launch = account_id.clone();
		let (launch, vault, launch_guard, selected_working_directory) =
			task::spawn_blocking(move || {
				let selected_working_directory = Arc::new(
					SelectedWorkingDirectory::acquire(&working_directory)
						.map_err(|()| QuickTaskManualRecovery::SelectWorkingDirectory)?,
				);
				let account_revision = credential.binding.account_revision;
				let binding = AccountBinding::shared_home_bound(
					account_id_for_launch.clone(),
					credential.binding,
					callback,
				)
				.map_err(|_| QuickTaskManualRecovery::ProcessUnavailable)?;
				let vault = QuickTaskCredentialVault {
					account_id: account_id_for_launch.clone(),
					stored: credential.stored,
				};
				let launch_guard = credential.launch_guard;
				let permit = capacity
					.reserve(account_id_for_launch, account_revision)
					.map_err(|_: CapacityExhausted| QuickTaskManualRecovery::ProcessUnavailable)?;
				let launch = AttestedAppServerLaunch::bind_selected_working_directory(
					profile,
					working_directory.into(),
					binding,
					PROCESS_TIMEOUT,
					permit,
				)
				.map_err(|_| QuickTaskManualRecovery::ProcessUnavailable)?;
				selected_working_directory
					.revalidate()
					.map_err(|()| QuickTaskManualRecovery::ProcessUnavailable)?;
				Ok::<_, QuickTaskManualRecovery>((
					launch,
					vault,
					launch_guard,
					selected_working_directory,
				))
			})
			.await
			.map_err(|_| QuickTaskManualRecovery::ProcessUnavailable)??;
		let mut process = spawn_admitted_quick_task_process(
			&self.inner.process_generations,
			admission,
			self.inner.execution_authorization.clone(),
			launch,
			selected_working_directory.clone(),
		)
		.await
		.map_err(|_| QuickTaskManualRecovery::ProcessUnavailable)?;
		let selected_working_directory_is_current =
			task::spawn_blocking(move || selected_working_directory.revalidate()).await;
		if !matches!(selected_working_directory_is_current, Ok(Ok(()))) {
			self.terminate_process(&process).await;
			return Err(QuickTaskManualRecovery::ProcessUnavailable);
		}
		drop(launch_guard);
		let process_for_init = process.clone();
		let control = self.inner.process_generations.clone();
		let initialized = task::spawn_blocking(move || {
			control.with_fenced_child(&process_for_init, |child| {
				child.initialize_ordinary_turns(&vault)
			})
		})
		.await;
		match initialized {
			Ok(Ok(Ok(()))) => {},
			Ok(Ok(Err(QuickTaskProcessError::Incompatible))) => {
				self.terminate_process(&process).await;
				return Err(QuickTaskManualRecovery::UpgradeCodex);
			},
			_ => {
				self.terminate_process(&process).await;
				return Err(QuickTaskManualRecovery::ProcessUnavailable);
			},
		}
		if self.inner.process_generations.mark_spawned_ready(&mut process).await.is_err() {
			self.terminate_process(&process).await;
			return Err(QuickTaskManualRecovery::ProcessUnavailable);
		}
		Ok(process)
	}

	async fn mark_active_unknown(&self, context: &TurnContext, ambiguity: QuickTaskAmbiguity) {
		let _ = self
			.inner
			.provider_attempts
			.mark_unknown(&context.attempt_id, context.authorized_revision)
			.await;
		self.terminate_process(&context.session.process).await;
		let readback = session_readback(
			&context.session,
			QuickTaskLocalState::OutcomeUnknown,
			Some(context.logical_turn_id.clone()),
		);
		self.set_recovery(readback.clone(), QuickTaskManualRecovery::MissingLocalProcess);
		self.emit(QuickTaskOutcome::Unknown { readback, ambiguity }).await;
	}

	async fn terminate_process(&self, process: &FencedProcess) {
		let _ = self.retire_process(process).await;
	}

	async fn retire_process(&self, process: &FencedProcess) -> bool {
		matches!(
			self
			.inner
			.process_generations
			.terminate_exact(process.generation_id(), process.revision(), Duration::from_secs(5))
			.await,
			Ok(
				ProcessGenerationTermination::PositiveDeathRecorded
					| ProcessGenerationTermination::AlreadyDead
			)
		)
	}

	fn reserve_initial(&self, command: &CreateQuickTask) -> Result<(), Box<QuickTaskOutcome>> {
		let mut local = self.local();
		if let Some(existing) = local.get(command.conversation_id.as_str()) {
			let readback = local_readback(&command.conversation_id, existing);
			return Err(Box::new(if existing.operation_key == command.operation_key {
				QuickTaskOutcome::Busy(readback)
			} else {
				QuickTaskOutcome::Conflict
			}));
		}
		if local.len() >= MAX_LOCAL_TASKS {
			return Err(Box::new(QuickTaskOutcome::Unavailable));
		}
		local.insert(
			command.conversation_id.as_str().to_owned(),
			LocalTask {
				operation_key: command.operation_key.clone(),
				state: LocalTaskState::Establishing,
			},
		);
		Ok(())
	}

	async fn submit_rehydrated_turn(
		&self,
		command: SubmitQuickTaskTurn,
		missing_local_process: QuickTaskOutcome,
	) -> QuickTaskOutcome {
		let admission = match self.admit_rehydrated_turn(&command, missing_local_process).await {
			Ok(admission) => admission,
			Err(outcome) => return *outcome,
		};
		let planned = match self.plan_rehydrated_turn(&command, admission).await {
			Ok(planned) => planned,
			Err(outcome) => return *outcome,
		};
		if planned.plan.plan.kind == decodex_core::ContinuationPlanKind::ContextPackFallback {
			let RehydratedTurnPlan { admission, decision, plan } = planned;
			return self
				.establish_context_fallback(command, admission.sequence, decision, plan, None)
				.await;
		}
		let account = match self.load_rehydrated_account_revision(&command, planned).await {
			Ok(account) => account,
			Err(outcome) => return *outcome,
		};
		let process_admission = match self.prepare_rehydrated_process(&command, account).await {
			Ok(admission) => admission,
			Err(outcome) => return *outcome,
		};
		let launch = match self.launch_rehydrated_process(&command, process_admission).await {
			Ok(launch) => launch,
			Err(outcome) => return *outcome,
		};
		self.resume_rehydrated_turn(command, launch).await
	}

	async fn admit_rehydrated_turn(
		&self,
		command: &SubmitQuickTaskTurn,
		missing_local_process: QuickTaskOutcome,
	) -> Result<RehydratedTurnAdmission, Box<QuickTaskOutcome>> {
		let readback = match self
			.inner
			.store
			.read_ordinary_runtime_session_for_resume(&command.conversation_id)
			.await
		{
			Ok(Some(readback)) => readback,
			Ok(None) => {
				return Err(Box::new(remap_recovery(
					missing_local_process,
					QuickTaskManualRecovery::MissingThread,
				)));
			},
			Err(
				StoreError::Incompatible(_)
				| StoreError::InvalidInput(_)
				| StoreError::CredentialRejected,
			) => {
				return Err(Box::new(remap_recovery(
					missing_local_process,
					QuickTaskManualRecovery::IncompatibleThread,
				)));
			},
			Err(error) => return Err(Box::new(store_outcome(error))),
		};
		let mut durable_readback = ordinary_resume_readback(&readback, command);
		if readback.has_active_turn {
			return Err(Box::new(QuickTaskOutcome::ManualRecovery {
				readback: durable_readback,
				action: QuickTaskManualRecovery::PriorActiveTurn,
			}));
		}
		if readback.has_unresolved_provider_attempt {
			return Err(Box::new(QuickTaskOutcome::ManualRecovery {
				readback: durable_readback,
				action: QuickTaskManualRecovery::PriorAttemptUnresolved,
			}));
		}

		let sequence = readback.next_turn_sequence;
		let turn_reservation = match self
			.reserve_user_turn(
				&command.operation_key,
				&command.conversation_id,
				&readback.runtime_session_id,
				&command.turn_id,
				sequence,
				&command.message,
			)
			.await
		{
			Ok(reservation) => reservation,
			Err(error) if turn_reservation_is_definite(&error) => {
				return Err(Box::new(QuickTaskOutcome::Conflict));
			},
			Err(error) if turn_reservation_is_integrity_failure(&error) => {
				durable_readback.active_turn_id = Some(command.turn_id.clone());
				return Err(Box::new(QuickTaskOutcome::ManualRecovery {
					readback: durable_readback,
					action: QuickTaskManualRecovery::PriorActiveTurn,
				}));
			},
			Err(_) => {
				durable_readback.active_turn_id = Some(command.turn_id.clone());
				let outcome =
					self.ambiguous(durable_readback, QuickTaskAmbiguity::TurnFinalization).await;
				return Err(Box::new(outcome));
			},
		};
		if !turn_admits_execution(&turn_reservation) {
			return Err(Box::new(QuickTaskOutcome::Conflict));
		}
		durable_readback.active_turn_id = Some(command.turn_id.clone());
		Ok(RehydratedTurnAdmission { readback, durable_readback, sequence })
	}

	async fn plan_rehydrated_turn(
		&self,
		command: &SubmitQuickTaskTurn,
		admission: RehydratedTurnAdmission,
	) -> Result<RehydratedTurnPlan, Box<QuickTaskOutcome>> {
		let consumer = ExecutionConsumer::ConversationTurn {
			conversation_id: command.conversation_id.clone(),
			conversation_revision: admission.readback.conversation_revision,
			source_runtime_session_id: Some(admission.readback.runtime_session_id.clone()),
			source_runtime_session_revision: Some(admission.readback.runtime_session_revision),
			turn_id: command.turn_id.clone(),
		};
		let planned = self
			.plan_existing_session(ExistingSessionPlanningInput {
				operation_key: &command.operation_key,
				consumer,
				message_bytes: command.message.len(),
				expected: ExistingSessionExpectation {
					account_id: &admission.readback.source_account_id,
					runtime_session_id: &admission.readback.runtime_session_id,
					runtime_session_revision: admission.readback.runtime_session_revision,
					thread_id: &admission.readback.codex_thread_id,
				},
			})
			.await;
		let (decision, plan) = match planned {
			Ok(planned) => planned,
			Err(ExistingSessionPlanningRefusal::Recovery(recovery)) => {
				let outcome = self
					.finalize_initial_refusal(
						&command.operation_key,
						&command.turn_id,
						admission.durable_readback,
						ReservedTurnRefusal::Recovery(recovery),
					)
					.await;
				return Err(Box::new(outcome));
			},
			Err(ExistingSessionPlanningRefusal::Conflict) => {
				let outcome = self
					.finalize_initial_refusal(
						&command.operation_key,
						&command.turn_id,
						admission.durable_readback,
						ReservedTurnRefusal::Conflict,
					)
					.await;
				return Err(Box::new(outcome));
			},
			Err(ExistingSessionPlanningRefusal::Unknown) => {
				let outcome = self
					.ambiguous(admission.durable_readback, QuickTaskAmbiguity::TurnFinalization)
					.await;
				return Err(Box::new(outcome));
			},
		};
		Ok(RehydratedTurnPlan { admission, decision, plan })
	}

	async fn load_rehydrated_account_revision(
		&self,
		command: &SubmitQuickTaskTurn,
		planned: RehydratedTurnPlan,
	) -> Result<RehydratedAccountRevision, Box<QuickTaskOutcome>> {
		match self.inner.accounts.inspect(&planned.admission.readback.source_account_id).await {
			Ok(inspection)
				if inspection.account.account_id
					== planned.admission.readback.source_account_id =>
				Ok(RehydratedAccountRevision {
					planned,
					launch_account_revision: inspection.account.revision,
				}),
			result => {
				let refusal = if result.is_ok() {
					ReservedTurnRefusal::Conflict
				} else {
					ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable)
				};
				let outcome = self
					.finalize_initial_refusal(
						&command.operation_key,
						&command.turn_id,
						planned.admission.durable_readback,
						refusal,
					)
					.await;
				Err(Box::new(outcome))
			},
		}
	}

	async fn prepare_rehydrated_process(
		&self,
		command: &SubmitQuickTaskTurn,
		account: RehydratedAccountRevision,
	) -> Result<RehydratedProcessAdmission, Box<QuickTaskOutcome>> {
		let working_directory = command.working_directory.clone();
		let generation_id = match ProcessGenerationId::new(derived_uuid(
			"rehydrated-process-generation",
			&[command.operation_key.as_str(), command.conversation_id.as_str()],
		)) {
			Ok(generation_id) => generation_id,
			Err(_) => {
				let outcome = self
					.finalize_initial_refusal(
						&command.operation_key,
						&command.turn_id,
						account.planned.admission.durable_readback,
						ReservedTurnRefusal::Unavailable,
					)
					.await;
				return Err(Box::new(outcome));
			},
		};
		let readback = &account.planned.admission.readback;
		let process_request = PrepareQuickTaskProcessGeneration {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: readback.conversation_revision,
			runtime_session_id: readback.runtime_session_id.clone(),
			expected_runtime_session_revision: readback.runtime_session_revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: account.planned.plan.plan.plan_id.clone(),
			routing_decision_id: account.planned.decision.decision_id.clone(),
			selected_account_id: readback.source_account_id.clone(),
			process_generation_id: generation_id.clone(),
		};
		let establishment = ReconcileQuickTaskThreadEstablishment {
			conversation_id: command.conversation_id.clone(),
			expected_conversation_revision: readback.conversation_revision,
			runtime_session_id: readback.runtime_session_id.clone(),
			expected_runtime_session_revision: readback.runtime_session_revision,
			turn_id: command.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: account.planned.plan.plan.plan_id.clone(),
			routing_decision_id: account.planned.decision.decision_id.clone(),
			selected_account_id: readback.source_account_id.clone(),
			process_generation_id: generation_id,
		};
		let admission = match self
			.inner
			.store
			.prepare_quick_task_process_generation(
				&scoped_key("process-admission", &command.operation_key),
				&process_request,
			)
			.await
		{
			Ok(PrepareQuickTaskProcessGenerationOutcome::Fresh(admission)) => admission,
			result => {
				let refusal = if matches!(
					result,
					Ok(PrepareQuickTaskProcessGenerationOutcome::Rejected(_))
				) {
					ReservedTurnRefusal::Conflict
				} else {
					ReservedTurnRefusal::Recovery(QuickTaskManualRecovery::ProcessUnavailable)
				};
				let outcome = self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						account.planned.admission.durable_readback,
						&establishment,
						refusal,
					)
					.await;
				return Err(Box::new(outcome));
			},
		};
		Ok(RehydratedProcessAdmission { account, working_directory, admission, establishment })
	}

	async fn launch_rehydrated_process(
		&self,
		command: &SubmitQuickTaskTurn,
		preparation: RehydratedProcessAdmission,
	) -> Result<RehydratedProcessLaunch, Box<QuickTaskOutcome>> {
		let RehydratedProcessAdmission { account, working_directory, admission, establishment } =
			preparation;
		let RehydratedAccountRevision { planned, launch_account_revision } = account;
		let RehydratedTurnPlan { admission: turn_admission, decision, plan } = planned;
		let RehydratedTurnAdmission { readback, durable_readback, sequence } = turn_admission;
		let process = match self
			.launch_process(
				&readback.source_account_id,
				launch_account_revision,
				admission,
				&working_directory,
			)
			.await
		{
			Ok(process) => process,
			Err(action) => {
				let outcome = self
					.reconcile_pre_effect(
						&command.operation_key,
						&command.turn_id,
						durable_readback,
						&establishment,
						ReservedTurnRefusal::Recovery(action),
					)
					.await;
				return Err(Box::new(outcome));
			},
		};
		let session = LocalSession {
			operation_key: command.operation_key.clone(),
			correlation_id: command.correlation_id.clone(),
			causation_id: command.causation_id.clone(),
			conversation_id: readback.conversation_id,
			conversation_revision: readback.conversation_revision,
			runtime_session_id: readback.runtime_session_id,
			runtime_session_revision: readback.runtime_session_revision,
			codex_thread_id: readback.codex_thread_id,
			has_acknowledged_turn: true,
			account_id: readback.source_account_id,
			process,
			model: command.execution.model.clone(),
			reasoning_effort: command.execution.reasoning_effort.clone(),
			fast: command.execution.fast,
			working_directory,
			instructions: readback.instructions,
			next_user_sequence: sequence.saturating_add(1),
		};
		Ok(RehydratedProcessLaunch { decision, plan, session, sequence })
	}

	async fn resume_rehydrated_turn(
		&self,
		command: SubmitQuickTaskTurn,
		launch: RehydratedProcessLaunch,
	) -> QuickTaskOutcome {
		let RehydratedProcessLaunch { decision, plan, session, sequence } = launch;
		let resume = match self.resume_same_thread(&session).await {
			Ok(resume) => resume,
			Err(SameThreadResumeRefusal::MissingThread) => {
				return self
					.finalize_bound_recovery(
						session,
						&command.turn_id,
						QuickTaskManualRecovery::MissingThread,
					)
					.await;
			},
			Err(SameThreadResumeRefusal::IncompatibleThread) => {
				return self
					.finalize_bound_recovery(
						session,
						&command.turn_id,
						QuickTaskManualRecovery::IncompatibleThread,
					)
					.await;
			},
			Err(SameThreadResumeRefusal::ProcessUnavailable) => {
				return self
					.finalize_bound_recovery(
						session,
						&command.turn_id,
						QuickTaskManualRecovery::ProcessUnavailable,
					)
					.await;
			},
			Err(SameThreadResumeRefusal::Ambiguous) => {
				return self
					.ambiguous_session(
						session,
						command.turn_id.clone(),
						QuickTaskAmbiguity::ThreadResume,
					)
					.await;
			},
		};
		if self
			.install_rehydrated_preparing(
				&command.operation_key,
				&command.conversation_id,
				&session,
			)
			.is_err()
		{
			let readback = session_readback(
				&session,
				QuickTaskLocalState::ManualRecovery,
				Some(command.turn_id.clone()),
			);
			self.terminate_process(&session.process).await;
			return self.ambiguous(readback, QuickTaskAmbiguity::ThreadResume).await;
		}
		self.dispatch_turn(
			&command.operation_key,
			decision,
			plan,
			session,
			command.turn_id,
			sequence,
			command.message,
			None,
			ProviderAttemptRuntimeAuthority::ExistingSessionResume(resume),
		)
		.await
	}

	fn install_rehydrated_preparing(
		&self,
		operation_key: &str,
		conversation_id: &ConversationId,
		session: &LocalSession,
	) -> Result<(), Box<QuickTaskOutcome>> {
		let mut local = self.local();
		if local.contains_key(conversation_id.as_str()) {
			return Err(Box::new(QuickTaskOutcome::Conflict));
		}
		if local.len() >= MAX_LOCAL_TASKS {
			return Err(Box::new(QuickTaskOutcome::Unavailable));
		}
		local.insert(
			conversation_id.as_str().to_owned(),
			LocalTask {
				operation_key: operation_key.to_owned(),
				state: LocalTaskState::Preparing(session.clone()),
			},
		);
		Ok(())
	}

	fn reserve_later_turn(
		&self,
		command: &SubmitQuickTaskTurn,
	) -> Result<LocalSession, Box<QuickTaskOutcome>> {
		let mut local = self.local();
		let Some(task) = local.get_mut(command.conversation_id.as_str()) else {
			return Err(Box::new(QuickTaskOutcome::ManualRecovery {
				readback: empty_readback(
					&command.conversation_id,
					QuickTaskLocalState::ManualRecovery,
				),
				action: QuickTaskManualRecovery::MissingLocalProcess,
			}));
		};
		let current = std::mem::replace(&mut task.state, LocalTaskState::Establishing);
		match current {
			LocalTaskState::Ready(mut session) => {
				if session.working_directory != command.working_directory {
					task.state = LocalTaskState::Ready(session);
					return Err(Box::new(QuickTaskOutcome::Conflict));
				}
				session.operation_key = command.operation_key.clone();
				session.correlation_id = command.correlation_id.clone();
				session.causation_id = command.causation_id.clone();
				task.operation_key = command.operation_key.clone();
				task.state = LocalTaskState::Preparing(session.clone());
				Ok(session)
			},
			other => {
				task.state = other;
				Err(Box::new(match &task.state {
					LocalTaskState::Preparing(session) => QuickTaskOutcome::Busy(session_readback(
						session,
						QuickTaskLocalState::Ready,
						None,
					)),
					LocalTaskState::Active { session, turn_id, .. } =>
						QuickTaskOutcome::Busy(session_readback(
							session,
							QuickTaskLocalState::Running,
							Some(turn_id.clone()),
						)),
					LocalTaskState::Recovery { readback, action } =>
						QuickTaskOutcome::ManualRecovery {
							readback: readback.clone(),
							action: *action,
						},
					LocalTaskState::Establishing | LocalTaskState::Ready(_) =>
						QuickTaskOutcome::Conflict,
				}))
			},
		}
	}

	fn set_preparing(&self, session: LocalSession) -> bool {
		let mut local = self.local();
		let Some(task) = local.get_mut(session.conversation_id.as_str()) else {
			return false;
		};
		let accepts = match &task.state {
			LocalTaskState::Establishing => true,
			LocalTaskState::Preparing(current) => same_local_process(current, &session),
			LocalTaskState::Ready(_)
			| LocalTaskState::Active { .. }
			| LocalTaskState::Recovery { .. } => false,
		};
		if accepts {
			task.state = LocalTaskState::Preparing(session);
		}
		accepts
	}

	fn replace_preparing_successor(
		&self,
		predecessor: &LocalSession,
		successor: LocalSession,
	) -> bool {
		let mut local = self.local();
		let Some(task) = local.get_mut(predecessor.conversation_id.as_str()) else {
			return false;
		};
		if !matches!(
			&task.state,
			LocalTaskState::Preparing(current) if same_local_process(current, predecessor)
		) || successor.conversation_id != predecessor.conversation_id
			|| successor.runtime_session_id == predecessor.runtime_session_id
		{
			return false;
		}
		task.operation_key = successor.operation_key.clone();
		task.state = LocalTaskState::Preparing(successor);
		true
	}

	fn set_active(
		&self,
		session: LocalSession,
		turn_id: TurnId,
		attempt_id: ProviderAttemptId,
		commands: mpsc::SyncSender<WorkerCommand>,
	) -> bool {
		let mut local = self.local();
		let Some(task) = local.get_mut(session.conversation_id.as_str()) else {
			return false;
		};
		if !matches!(
			&task.state,
			LocalTaskState::Preparing(current) if same_local_process(current, &session)
		) {
			return false;
		}
		task.state = LocalTaskState::Active { session, turn_id, attempt_id, commands };
		true
	}

	fn restore_ready(&self, session: LocalSession) {
		let mut local = self.local();
		if let Some(task) = local.get_mut(session.conversation_id.as_str()) {
			task.state = LocalTaskState::Ready(session);
		}
	}

	fn remove_active_if(
		&self,
		attempt_id: &ProviderAttemptId,
		conversation_id: &ConversationId,
	) -> bool {
		let mut local = self.local();
		let belongs = local.get(conversation_id.as_str()).is_some_and(|task| {
			matches!(
				&task.state,
				LocalTaskState::Active { attempt_id: active_attempt_id, .. }
					if active_attempt_id == attempt_id
			)
		});
		if belongs {
			local.remove(conversation_id.as_str());
		}
		belongs
	}

	fn set_recovery_if_active(
		&self,
		attempt_id: &ProviderAttemptId,
		readback: QuickTaskReadback,
		action: QuickTaskManualRecovery,
	) -> bool {
		let mut local = self.local();
		let Some(task) = local.get_mut(readback.conversation_id.as_str()) else {
			return false;
		};
		if !matches!(
			&task.state,
			LocalTaskState::Active { attempt_id: active_attempt_id, .. }
				if active_attempt_id == attempt_id
		) {
			return false;
		}
		task.state = LocalTaskState::Recovery { readback, action };
		true
	}

	fn stop_active_worker(&self, attempt_id: &ProviderAttemptId, conversation_id: &ConversationId) {
		let local = self.local();
		if let Some(task) = local.get(conversation_id.as_str())
			&& let LocalTaskState::Active { attempt_id: active_attempt_id, commands, .. } =
				&task.state
			&& active_attempt_id == attempt_id
		{
			let _ = commands.try_send(WorkerCommand::Shutdown);
		}
	}

	fn set_recovery(&self, readback: QuickTaskReadback, action: QuickTaskManualRecovery) {
		let mut local = self.local();
		if let Some(task) = local.get_mut(readback.conversation_id.as_str()) {
			task.state = LocalTaskState::Recovery { readback, action };
		}
	}

	async fn cancel_prepared_exact(
		&self,
		attempt_id: &ProviderAttemptId,
		prepared_revision: i64,
	) -> PreparedCancellationDisposition {
		let Some(canceled_revision) = prepared_revision.checked_add(1) else {
			return PreparedCancellationDisposition::Conflict;
		};
		match self.inner.provider_attempts.cancel_prepared(attempt_id, prepared_revision).await {
			Ok(
				ProviderAttemptMutationOutcome::Applied(mutation)
				| ProviderAttemptMutationOutcome::Replayed(mutation),
			) if mutation.state == ProviderAttemptState::Canceled
				&& mutation.revision == canceled_revision =>
				PreparedCancellationDisposition::Canceled,
			Ok(ProviderAttemptMutationOutcome::Rejected { actual, .. })
				if matches!(
					actual.state,
					ProviderAttemptState::DispatchAuthorized | ProviderAttemptState::Unknown
				) || actual.state.is_terminal() =>
			{
				if actual.state == ProviderAttemptState::DispatchAuthorized {
					let _ = self
						.inner
						.provider_attempts
						.mark_unknown(attempt_id, actual.revision)
						.await;
				}
				PreparedCancellationDisposition::Ambiguous
			},
			Ok(_) => PreparedCancellationDisposition::Conflict,
			Err(_) => PreparedCancellationDisposition::Ambiguous,
		}
	}

	async fn recover_active_turn(
		&self,
		session: LocalSession,
		turn_id: TurnId,
		action: QuickTaskManualRecovery,
	) -> QuickTaskOutcome {
		self.terminate_process(&session.process).await;
		self.recover(
			session_readback(&session, QuickTaskLocalState::ManualRecovery, Some(turn_id)),
			action,
		)
		.await
	}

	async fn finalize_reserved_turn(
		&self,
		operation_key: &str,
		turn_id: &TurnId,
	) -> Result<(), ()> {
		let command =
			exact_command("turn-refusal", operation_key, &[turn_id.as_str(), "1", "failed"])?;
		let revision = self
			.inner
			.store
			.transition_turn(&command, turn_id, 1, decodex_core::TurnStatus::Failed)
			.await
			.map_err(|_| ())?;
		if revision != 2 {
			return Err(());
		}
		Ok(())
	}

	async fn finalize_bound_refusal(
		&self,
		session: LocalSession,
		turn_id: &TurnId,
		refusal: ReservedTurnRefusal,
	) -> QuickTaskOutcome {
		if self.finalize_reserved_turn(&session.operation_key, turn_id).await.is_err() {
			return self
				.ambiguous_session(session, turn_id.clone(), QuickTaskAmbiguity::TurnFinalization)
				.await;
		}
		if !session.has_acknowledged_turn {
			return self.recover_session(session, QuickTaskManualRecovery::MissingThread).await;
		}
		self.restore_ready(session.clone());
		match refusal {
			ReservedTurnRefusal::Conflict => QuickTaskOutcome::Conflict,
			ReservedTurnRefusal::Unavailable => QuickTaskOutcome::Unavailable,
			ReservedTurnRefusal::Recovery(action) => QuickTaskOutcome::ManualRecovery {
				readback: session_readback(&session, QuickTaskLocalState::Ready, None),
				action,
			},
		}
	}

	async fn finalize_bound_recovery(
		&self,
		session: LocalSession,
		turn_id: &TurnId,
		action: QuickTaskManualRecovery,
	) -> QuickTaskOutcome {
		if self.finalize_reserved_turn(&session.operation_key, turn_id).await.is_err() {
			return self
				.ambiguous_session(session, turn_id.clone(), QuickTaskAmbiguity::TurnFinalization)
				.await;
		}
		self.recover_session(session, action).await
	}

	async fn reconcile_pre_effect(
		&self,
		operation_key: &str,
		turn_id: &TurnId,
		mut readback: QuickTaskReadback,
		coordinates: &ReconcileQuickTaskThreadEstablishment,
		refusal: ReservedTurnRefusal,
	) -> QuickTaskOutcome {
		readback.process_generation_id = Some(coordinates.process_generation_id.clone());
		match self.inner.store.reconcile_quick_task_thread_establishment(coordinates).await {
			Ok(QuickTaskThreadEstablishmentReadback::DefinitelyNotStarted(_)) =>
				self.finalize_initial_refusal(operation_key, turn_id, readback, refusal).await,
			Ok(QuickTaskThreadEstablishmentReadback::Fenced(fence)) => {
				readback.runtime_session_revision = Some(fence.revision);
				self.ambiguous(readback, QuickTaskAmbiguity::ThreadStart).await
			},
			Ok(QuickTaskThreadEstablishmentReadback::Bound(binding)) => {
				readback.runtime_session_revision = Some(binding.revision);
				readback.codex_thread_id = Some(binding.codex_thread_id);
				self.ambiguous(readback, QuickTaskAmbiguity::ThreadBind).await
			},
			Ok(QuickTaskThreadEstablishmentReadback::Unknown) | Err(_) =>
				self.ambiguous(readback, QuickTaskAmbiguity::ProcessGeneration).await,
		}
	}

	async fn finalize_initial_refusal(
		&self,
		operation_key: &str,
		turn_id: &TurnId,
		readback: QuickTaskReadback,
		refusal: ReservedTurnRefusal,
	) -> QuickTaskOutcome {
		if self.finalize_reserved_turn(operation_key, turn_id).await.is_err() {
			return self.ambiguous(readback, QuickTaskAmbiguity::TurnFinalization).await;
		}
		match refusal {
			ReservedTurnRefusal::Conflict => QuickTaskOutcome::Conflict,
			ReservedTurnRefusal::Unavailable => QuickTaskOutcome::Unavailable,
			ReservedTurnRefusal::Recovery(action) => self.recover(readback, action).await,
		}
	}

	async fn recover(
		&self,
		mut readback: QuickTaskReadback,
		action: QuickTaskManualRecovery,
	) -> QuickTaskOutcome {
		readback.state = QuickTaskLocalState::ManualRecovery;
		self.set_recovery(readback.clone(), action);
		QuickTaskOutcome::ManualRecovery { readback, action }
	}

	async fn recover_session(
		&self,
		session: LocalSession,
		action: QuickTaskManualRecovery,
	) -> QuickTaskOutcome {
		self.terminate_process(&session.process).await;
		self.recover(session_readback(&session, QuickTaskLocalState::ManualRecovery, None), action)
			.await
	}

	fn pre_session(
		&self,
		command: &CreateQuickTask,
		conversation_revision: i64,
		state: QuickTaskLocalState,
	) -> QuickTaskOutcome {
		QuickTaskOutcome::PreSession(QuickTaskReadback {
			operation_key: Some(command.operation_key.clone()),
			correlation_id: Some(command.correlation_id.clone()),
			causation_id: command.causation_id.clone(),
			conversation_id: command.conversation_id.clone(),
			conversation_revision: Some(conversation_revision),
			runtime_session_id: None,
			runtime_session_revision: None,
			codex_thread_id: None,
			process_generation_id: None,
			active_turn_id: None,
			state,
		})
	}

	async fn ambiguous(
		&self,
		mut readback: QuickTaskReadback,
		ambiguity: QuickTaskAmbiguity,
	) -> QuickTaskOutcome {
		readback.state = QuickTaskLocalState::OutcomeUnknown;
		self.set_recovery(readback.clone(), QuickTaskManualRecovery::MissingLocalProcess);
		QuickTaskOutcome::Unknown { readback, ambiguity }
	}

	async fn ambiguous_session(
		&self,
		session: LocalSession,
		active_turn_id: TurnId,
		ambiguity: QuickTaskAmbiguity,
	) -> QuickTaskOutcome {
		self.terminate_process(&session.process).await;
		self.ambiguous(
			session_readback(&session, QuickTaskLocalState::OutcomeUnknown, Some(active_turn_id)),
			ambiguity,
		)
		.await
	}

	async fn emit(&self, outcome: QuickTaskOutcome) {
		let _ = self.inner.events.send(outcome).await;
	}

	fn remove_establishing(&self, conversation_id: &ConversationId) {
		let mut local = self.local();
		if local
			.get(conversation_id.as_str())
			.is_some_and(|task| matches!(&task.state, LocalTaskState::Establishing))
		{
			local.remove(conversation_id.as_str());
		}
	}

	fn is_shutting_down(&self) -> bool {
		self.inner.shutting_down.load(std::sync::atomic::Ordering::Acquire)
	}

	fn local(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, LocalTask>> {
		self.inner.local.lock().unwrap_or_else(PoisonError::into_inner)
	}
}

fn run_event_worker(
	control: ProcessGenerationControl,
	process: FencedProcess,
	thread_id: String,
	provider_turn_id: String,
	commands: mpsc::Receiver<WorkerCommand>,
	shutting_down: Arc<std::sync::atomic::AtomicBool>,
	output: tokio_mpsc::Sender<WorkerOutput>,
) {
	let result = control.with_fenced_child(&process, |child| {
		run_event_loop(child, thread_id, provider_turn_id, commands, &shutting_down, &output)
	});
	if !matches!(result, Ok(Ok(()))) {
		let _ = output.blocking_send(WorkerOutput::Failed);
	}
}

fn run_event_loop(
	child: &mut AttestedProcessChild,
	thread_id: String,
	provider_turn_id: String,
	commands: mpsc::Receiver<WorkerCommand>,
	shutting_down: &std::sync::atomic::AtomicBool,
	output: &tokio_mpsc::Sender<WorkerOutput>,
) -> Result<(), QuickTaskProcessError> {
	let thread_id =
		ExactThreadId::new(thread_id).map_err(|_| QuickTaskProcessError::Incompatible)?;
	let provider_turn_id = decodex_codex::ExactTurnId::new(provider_turn_id)
		.map_err(|_| QuickTaskProcessError::Incompatible)?;
	let deadline = Instant::now() + TURN_TIMEOUT;
	loop {
		if shutting_down.load(std::sync::atomic::Ordering::Acquire) || Instant::now() >= deadline {
			let request =
				QuickTaskTurnInterruptRequest::new(thread_id.clone(), provider_turn_id.clone());
			let _ = child.interrupt_ordinary_turn(&request);
			return Err(QuickTaskProcessError::Unavailable);
		}
		match commands.try_recv() {
			Ok(WorkerCommand::Interrupt) => {
				let request =
					QuickTaskTurnInterruptRequest::new(thread_id.clone(), provider_turn_id.clone());
				for event in child.interrupt_ordinary_turn(&request)? {
					output
						.blocking_send(WorkerOutput::Event(event))
						.map_err(|_| QuickTaskProcessError::Unavailable)?;
				}
			},
			Ok(WorkerCommand::Shutdown) => {
				let request =
					QuickTaskTurnInterruptRequest::new(thread_id.clone(), provider_turn_id.clone());
				let _ = child.interrupt_ordinary_turn(&request);
				return Err(QuickTaskProcessError::Unavailable);
			},
			Err(mpsc::TryRecvError::Empty) => {},
			Err(mpsc::TryRecvError::Disconnected) => {
				let request =
					QuickTaskTurnInterruptRequest::new(thread_id.clone(), provider_turn_id.clone());
				let _ = child.interrupt_ordinary_turn(&request);
				return Err(QuickTaskProcessError::Unavailable);
			},
		}
		if let Some(event) = child.next_ordinary_turn_event(EVENT_POLL)? {
			let terminal = matches!(&event, QuickTaskProcessEvent::TurnCompleted { .. });
			output
				.blocking_send(WorkerOutput::Event(event))
				.map_err(|_| QuickTaskProcessError::Unavailable)?;
			if terminal {
				return Ok(());
			}
		}
	}
}

struct QuickTaskCredentialVault {
	account_id: AccountId,
	stored: crate::StoredCredential,
}

impl CredentialVault for QuickTaskCredentialVault {
	fn project(
		&self,
		account_id: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		if account_id != &self.account_id {
			return Err(CredentialVaultError::Unavailable);
		}
		let binding = self.stored.binding();
		let bundle = self.stored.bundle();
		projection.authenticate_chatgpt(
			bundle.access_token(),
			binding.provider.account_id(),
			bundle.plan_type(),
		)?;
		Ok(AccountIdentity::from_observation("chatgpt", Some(bundle.provider_email()), true))
	}
}

impl Debug for QuickTaskCredentialVault {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("QuickTaskCredentialVault([REDACTED])")
	}
}

fn same_local_process(left: &LocalSession, right: &LocalSession) -> bool {
	left.conversation_id == right.conversation_id
		&& left.runtime_session_id == right.runtime_session_id
		&& left.runtime_session_revision == right.runtime_session_revision
		&& left.codex_thread_id == right.codex_thread_id
		&& left.has_acknowledged_turn == right.has_acknowledged_turn
		&& left.account_id == right.account_id
		&& left.process.generation_id() == right.process.generation_id()
		&& left.process.revision() == right.process.revision()
		&& left.process.identity() == right.process.identity()
}

struct QuickTaskRefreshCallback {
	accounts: Arc<AccountService>,
	runtime: tokio::runtime::Handle,
	generation_id: ProcessGenerationId,
}

impl ProcessAccountRefreshCallback for QuickTaskRefreshCallback {
	fn refresh(
		&self,
		account_id: &AccountId,
		initial_binding: &ProcessGenerationAccountBinding,
		reason: &str,
		previous_provider_account_id: Option<&str>,
	) -> Result<ChatgptRefreshProjection, CredentialVaultError> {
		if reason != "unauthorized" {
			return Err(CredentialVaultError::ProjectionRejected);
		}
		let operation_id =
			AccountOperationId::generate().map_err(|_| CredentialVaultError::Unavailable)?;
		let projection = self
			.runtime
			.block_on(self.accounts.refresh(
				operation_id,
				account_id,
				None,
				Some((&self.generation_id, initial_binding)),
				previous_provider_account_id,
			))
			.map_err(|_| CredentialVaultError::Unavailable)?;
		ChatgptRefreshProjection::new(
			projection.access_token().to_owned(),
			projection.provider_account_id().to_owned(),
			projection.plan_type().map(str::to_owned),
		)
	}
}

fn ordinary_resume_readback(
	readback: &OrdinaryRuntimeSessionResumeReadback,
	command: &SubmitQuickTaskTurn,
) -> QuickTaskReadback {
	QuickTaskReadback {
		operation_key: Some(command.operation_key.clone()),
		correlation_id: Some(command.correlation_id.clone()),
		causation_id: command.causation_id.clone(),
		conversation_id: readback.conversation_id.clone(),
		conversation_revision: Some(readback.conversation_revision),
		runtime_session_id: Some(readback.runtime_session_id.clone()),
		runtime_session_revision: Some(readback.runtime_session_revision),
		codex_thread_id: Some(readback.codex_thread_id.clone()),
		process_generation_id: None,
		active_turn_id: None,
		state: QuickTaskLocalState::ManualRecovery,
	}
}

fn session_readback(
	session: &LocalSession,
	state: QuickTaskLocalState,
	active_turn_id: Option<TurnId>,
) -> QuickTaskReadback {
	QuickTaskReadback {
		operation_key: Some(session.operation_key.clone()),
		correlation_id: Some(session.correlation_id.clone()),
		causation_id: session.causation_id.clone(),
		conversation_id: session.conversation_id.clone(),
		conversation_revision: Some(session.conversation_revision),
		runtime_session_id: Some(session.runtime_session_id.clone()),
		runtime_session_revision: Some(session.runtime_session_revision),
		codex_thread_id: Some(session.codex_thread_id.clone()),
		process_generation_id: Some(session.process.generation_id().clone()),
		active_turn_id,
		state,
	}
}

fn remap_recovery(outcome: QuickTaskOutcome, action: QuickTaskManualRecovery) -> QuickTaskOutcome {
	match outcome {
		QuickTaskOutcome::ManualRecovery { readback, .. } =>
			QuickTaskOutcome::ManualRecovery { readback, action },
		outcome => outcome,
	}
}

fn empty_readback(
	conversation_id: &ConversationId,
	state: QuickTaskLocalState,
) -> QuickTaskReadback {
	QuickTaskReadback {
		operation_key: None,
		correlation_id: None,
		causation_id: None,
		conversation_id: conversation_id.clone(),
		conversation_revision: None,
		runtime_session_id: None,
		runtime_session_revision: None,
		codex_thread_id: None,
		process_generation_id: None,
		active_turn_id: None,
		state,
	}
}

fn local_readback(conversation_id: &ConversationId, task: &LocalTask) -> QuickTaskReadback {
	match &task.state {
		LocalTaskState::Establishing =>
			empty_readback(conversation_id, QuickTaskLocalState::Establishing),
		LocalTaskState::Preparing(session) =>
			session_readback(session, QuickTaskLocalState::Ready, None),
		LocalTaskState::Ready(session) =>
			session_readback(session, QuickTaskLocalState::Ready, None),
		LocalTaskState::Active { session, turn_id, .. } =>
			session_readback(session, QuickTaskLocalState::Running, Some(turn_id.clone())),
		LocalTaskState::Recovery { readback, .. } => readback.clone(),
	}
}

fn account_recovery(error: AccountLifecycleError) -> QuickTaskManualRecovery {
	match error {
		AccountLifecycleError::AccountDisabled => QuickTaskManualRecovery::EnableAccount,
		AccountLifecycleError::CredentialAbsent
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent) =>
			QuickTaskManualRecovery::EnrollCredentials,
		AccountLifecycleError::CredentialStore(_)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::StoreUnavailable)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::StoreMismatch) =>
			QuickTaskManualRecovery::RepairCredentialStore,
		AccountLifecycleError::OperationRejected(_)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled) =>
			QuickTaskManualRecovery::ResolveAccountOperation,
		AccountLifecycleError::StaleAccount => QuickTaskManualRecovery::SelectedAccountDrift,
		AccountLifecycleError::ProviderMismatch
		| AccountLifecycleError::Refresh(_)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch) =>
			QuickTaskManualRecovery::RestoreProviderAgreement,
		AccountLifecycleError::NotReady(AccountLifecycleReadiness::CallbackCapabilityUnready) =>
			QuickTaskManualRecovery::UpgradeCodex,
		AccountLifecycleError::AccountMissing
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned) =>
			QuickTaskManualRecovery::MissingLocalProcess,
		AccountLifecycleError::Persistence(_)
		| AccountLifecycleError::CoordinatorUnavailable
		| AccountLifecycleError::InvalidOperation
		| AccountLifecycleError::CredentialImport
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::Ready) =>
			QuickTaskManualRecovery::ProcessUnavailable,
	}
}

fn turn_admits_execution(reservation: &TurnReservationOutcome) -> bool {
	match reservation {
		TurnReservationOutcome::Fresh(readback) =>
			readback.status == decodex_core::TurnStatus::Active && readback.revision == 1,
		TurnReservationOutcome::Replayed(readback) =>
			readback.status == decodex_core::TurnStatus::Active && readback.revision == 1,
	}
}

fn turn_reservation_is_definite(error: &StoreError) -> bool {
	matches!(
		error,
		StoreError::IdempotencyConflict
			| StoreError::OperationIdConflict
			| StoreError::RevisionConflict { .. }
			| StoreError::CredentialRejected
			| StoreError::InvalidInput(_)
			| StoreError::CapacityExhausted(_)
	)
}

fn turn_reservation_is_integrity_failure(error: &StoreError) -> bool {
	matches!(error, StoreError::Incompatible(_) | StoreError::UnsafeHostPath)
}

fn store_outcome(error: decodex_database::StoreError) -> QuickTaskOutcome {
	match error {
		decodex_database::StoreError::IdempotencyConflict
		| decodex_database::StoreError::OperationIdConflict
		| decodex_database::StoreError::RevisionConflict { .. }
		| decodex_database::StoreError::InvalidInput(_)
		| decodex_database::StoreError::CredentialRejected => QuickTaskOutcome::Conflict,
		_ => QuickTaskOutcome::Unavailable,
	}
}

fn history_chunk_end(text: &str, start: usize) -> usize {
	let mut end = start.saturating_add(MAX_HISTORY_INLINE_BYTES).min(text.len());
	while end > start && !text.is_char_boundary(end) {
		end -= 1;
	}
	end
}

fn markdown_media_type() -> HistoryMediaType {
	HistoryMediaType::new("text/markdown").expect("constant media type is valid")
}

fn exact_command(scope: &str, key: &str, parts: &[&str]) -> Result<CommandIdentity, ()> {
	CommandIdentity::new(scoped_key(scope, key), request_digest(parts).as_bytes()).map_err(|_| ())
}

fn scoped_key(scope: &str, key: &str) -> String {
	format!("ordinary-{scope}:{}", request_digest(&[key]))
}

fn request_digest(parts: &[&str]) -> String {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update(part.len().to_be_bytes());
		digest.update(part.as_bytes());
	}
	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn derived_uuid(scope: &str, parts: &[&str]) -> String {
	let digest = Sha256::digest(
		format!("decodex/ordinary-task/{scope}/{}", request_digest(parts)).as_bytes(),
	);
	let mut bytes = [0_u8; 16];
	bytes.copy_from_slice(&digest[..16]);
	bytes[6] = (bytes[6] & 0x0f) | 0x40;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		bytes[0],
		bytes[1],
		bytes[2],
		bytes[3],
		bytes[4],
		bytes[5],
		bytes[6],
		bytes[7],
		bytes[8],
		bytes[9],
		bytes[10],
		bytes[11],
		bytes[12],
		bytes[13],
		bytes[14],
		bytes[15],
	)
}

#[cfg(test)]
mod tests {
	use decodex_core::{ContinuationRejection, TurnId, TurnStatus};
	use decodex_database::{TurnReservationOutcome, TurnReservationReadback};

	use super::{
		QuickTaskManualRecovery, account_recovery, continuation_recovery, derived_uuid,
		request_digest, scoped_key, turn_admits_execution,
	};
	use crate::account_service::AccountLifecycleError;

	fn reservation(status: TurnStatus, revision: i64) -> TurnReservationReadback {
		TurnReservationReadback {
			turn_id: TurnId::new(derived_uuid("test-turn", &["ordinary-task"]))
				.expect("derived test Turn UUID is valid"),
			sequence: 1,
			status,
			revision,
		}
	}

	#[test]
	fn only_active_revision_one_turn_authority_admits_execution() {
		assert!(turn_admits_execution(&TurnReservationOutcome::Fresh(reservation(
			TurnStatus::Active,
			1,
		))));
		assert!(turn_admits_execution(&TurnReservationOutcome::Replayed(reservation(
			TurnStatus::Active,
			1,
		))));
		assert!(!turn_admits_execution(&TurnReservationOutcome::Fresh(reservation(
			TurnStatus::Active,
			2,
		))));
		assert!(!turn_admits_execution(&TurnReservationOutcome::Fresh(reservation(
			TurnStatus::Completed,
			1,
		))));
	}

	#[test]
	fn ordinary_task_identities_are_deterministic_and_scope_separated() {
		let digest = request_digest(&["account", "conversation"]);
		assert_eq!(digest, request_digest(&["account", "conversation"]));
		assert_ne!(digest, request_digest(&["conversation", "account"]));
		assert_ne!(scoped_key("route", "command"), scoped_key("continue", "command"));

		let first = derived_uuid("provider-attempt", &["account", "conversation"]);
		assert_eq!(first, derived_uuid("provider-attempt", &["account", "conversation"]));
		assert_ne!(first, derived_uuid("runtime-session", &["account", "conversation"]));
		assert_eq!(first.as_bytes()[14], b'4');
		assert!(matches!(first.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
	}

	#[test]
	fn account_failures_preserve_typed_manual_recovery() {
		assert_eq!(
			account_recovery(AccountLifecycleError::AccountDisabled),
			QuickTaskManualRecovery::EnableAccount
		);
		assert_eq!(
			account_recovery(AccountLifecycleError::ProviderMismatch),
			QuickTaskManualRecovery::RestoreProviderAgreement
		);
		assert_eq!(
			account_recovery(AccountLifecycleError::StaleAccount),
			QuickTaskManualRecovery::SelectedAccountDrift
		);
		assert_eq!(
			account_recovery(AccountLifecycleError::AccountMissing),
			QuickTaskManualRecovery::MissingLocalProcess
		);
	}

	#[test]
	fn continuation_account_recovery_is_typed_without_reselection() {
		assert_eq!(
			continuation_recovery(ContinuationRejection::SelectedAccountDrift),
			Some(QuickTaskManualRecovery::SelectedAccountDrift),
		);
		assert_eq!(
			continuation_recovery(ContinuationRejection::SelectedAccountReadinessRequired),
			Some(QuickTaskManualRecovery::SelectedAccountReadiness),
		);
		assert_eq!(
			continuation_recovery(ContinuationRejection::SelectedAccountQuotaRequired),
			Some(QuickTaskManualRecovery::RefreshQuota),
		);
		assert_eq!(continuation_recovery(ContinuationRejection::SameThreadUnavailable), None);
	}
}
