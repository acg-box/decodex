//! Bounded presentation facts for ordinary Task-role conversations.

use std::{
	collections::HashSet,
	fmt::{Debug, Display, Formatter},
};

pub use decodex_core::MAX_PROVIDER_THREAD_ID_BYTES;
use decodex_core::{WorkItemState, contains_credential_material};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use url::Url;

use crate::{EntityId, EntityRevision, WireText};

/// Maximum ordinary Task conversations returned by one list observation.
pub const MAX_CONVERSATION_LIST_SIZE: u16 = 64;
/// Maximum UTF-8 bytes in one authoritative user-visible Conversation title.
pub const MAX_CONVERSATION_TITLE_BYTES: usize = 96;
/// Maximum UTF-8 bytes in one server-host Conversation working-directory input.
pub const MAX_CONVERSATION_WORKING_DIRECTORY_BYTES: usize = 4_096;
/// Maximum UTF-8 bytes in one explicit Codex model identifier.
pub const MAX_CONVERSATION_MODEL_BYTES: usize = 128;

/// Closed, redacted reason that Conversation execution was unavailable at daemon startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationUnavailableReason {
	/// Exact local product-state authority was unavailable.
	ProductState,
	/// Content-addressed blob storage was unavailable.
	BlobStore,
	/// Account Service and its credential owner were unavailable.
	AccountService,
	/// ProcessGeneration startup or reconciliation was unavailable.
	ProcessGeneration,
	/// ProviderAttempt startup or reconciliation was unavailable.
	ProviderAttempt,
	/// The owner-only process execution epoch was unavailable.
	ExecutionAuthorization,
	/// The exact Codex app-server profile or callback contract was unavailable.
	AppServerProfile,
	/// Bounded daemon process capacity could not be constructed.
	RunnerCapacity,
	/// This host platform cannot execute the accepted Conversation process contract.
	UnsupportedPlatform,
}

/// Invalid bounded Conversation presentation input or projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationContractError {
	/// A working-directory input was not a bounded normalized absolute path.
	InvalidWorkingDirectory,
	/// A requested list size was zero or exceeded the public bound.
	InvalidListSize,
	/// A list cursor was not a positive canonical ordinary Conversation position.
	InvalidCursor,
	/// A model identifier was empty, oversized, or not a safe protocol label.
	InvalidModel,
	/// An ordinary Conversation or RuntimeSession projection was internally inconsistent.
	InvalidProjection,
}

/// Bounded credential-negative title persisted by Conversation authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConversationTitle(String);
impl ConversationTitle {
	/// Validate and retain one authoritative display title.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > MAX_CONVERSATION_TITLE_BYTES
			|| value.chars().any(char::is_control)
			|| contains_credential_material(&value)
		{
			return Err(ConversationContractError::InvalidProjection);
		}
		Ok(Self(value))
	}

	/// Borrow the exact validated title.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl<'de> Deserialize<'de> for ConversationTitle {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Exact opaque Codex app-server thread identity after authoritative readback.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProviderThreadId(String);
impl ProviderThreadId {
	/// Validate and retain one exact opaque app-server thread identity.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > MAX_PROVIDER_THREAD_ID_BYTES
			|| value.chars().any(char::is_control)
			|| value.contains(['"', '\\'])
		{
			return Err(ConversationContractError::InvalidProjection);
		}
		Ok(Self(value))
	}

	/// Borrow the exact provider identity byte-for-byte.
	pub fn as_str(&self) -> &str {
		&self.0
	}

	/// Project this exact identity as one percent-encoded `codex://threads/` path segment.
	pub fn codex_url(&self) -> Result<Url, ConversationContractError> {
		let mut url = Url::parse("codex://threads")
			.map_err(|_| ConversationContractError::InvalidProjection)?;
		url.path_segments_mut()
			.map_err(|()| ConversationContractError::InvalidProjection)?
			.push(&self.0);
		Ok(url)
	}

	/// Recover the exact provider identity from one canonical Codex thread URL.
	pub fn from_codex_url(url: &Url) -> Result<Self, ConversationContractError> {
		if url.scheme() != "codex"
			|| url.host_str() != Some("threads")
			|| !url.username().is_empty()
			|| url.password().is_some()
			|| url.port().is_some()
			|| url.query().is_some()
			|| url.fragment().is_some()
		{
			return Err(ConversationContractError::InvalidProjection);
		}
		let segments = url
			.path_segments()
			.ok_or(ConversationContractError::InvalidProjection)?
			.collect::<Vec<_>>();
		let [segment] = segments.as_slice() else {
			return Err(ConversationContractError::InvalidProjection);
		};
		let decoded = percent_decode_str(segment)
			.decode_utf8()
			.map_err(|_| ConversationContractError::InvalidProjection)?;
		Self::new(decoded.into_owned())
	}
}
impl<'de> Deserialize<'de> for ProviderThreadId {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Exact persisted Program WorkItem context for one bound Conversation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationProgramContext {
	/// Owning persisted Program identity.
	pub program_id: EntityId,
	/// Bound persisted Program WorkItem identity.
	pub work_item_id: EntityId,
	/// Authoritative display title shared with the Conversation.
	pub title: ConversationTitle,
	/// Exact persisted WorkItem instructions.
	pub instructions: WireText,
	/// Current persisted WorkItem lifecycle state.
	pub state: WorkItemState,
	/// Exact persisted WorkItem revision.
	pub revision: EntityRevision,
}
impl ConversationProgramContext {
	/// Validate one exact persisted Program WorkItem projection.
	pub fn new(
		program_id: EntityId,
		work_item_id: EntityId,
		title: ConversationTitle,
		instructions: WireText,
		state: WorkItemState,
		revision: EntityRevision,
	) -> Result<Self, ConversationContractError> {
		if !is_canonical_uuid_v4(program_id.as_str())
			|| !is_canonical_uuid_v4(work_item_id.as_str())
			|| instructions.as_str().is_empty()
			|| instructions.as_str().chars().any(char::is_control)
			|| revision.0 == 0
		{
			return Err(ConversationContractError::InvalidProjection);
		}
		Ok(Self { program_id, work_item_id, title, instructions, state, revision })
	}
}
impl<'de> Deserialize<'de> for ConversationProgramContext {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			program_id: EntityId,
			work_item_id: EntityId,
			title: ConversationTitle,
			instructions: WireText,
			state: WorkItemState,
			revision: EntityRevision,
		}
		let raw = Raw::deserialize(deserializer)?;
		Self::new(
			raw.program_id,
			raw.work_item_id,
			raw.title,
			raw.instructions,
			raw.state,
			raw.revision,
		)
		.map_err(D::Error::custom)
	}
}
impl Display for ConversationContractError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidWorkingDirectory => "invalid Conversation working directory",
			Self::InvalidListSize => "invalid Conversation list size",
			Self::InvalidCursor => "invalid Conversation list cursor",
			Self::InvalidModel => "invalid Conversation model",
			Self::InvalidProjection => "invalid Conversation conversation projection",
		})
	}
}

/// Bounded explicit Codex model used for the next Conversation send.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConversationModel(String);
impl ConversationModel {
	/// Validate one model identifier without selecting a default.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > MAX_CONVERSATION_MODEL_BYTES
			|| value.chars().any(|character| {
				character.is_control()
					|| !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
			}) {
			return Err(ConversationContractError::InvalidModel);
		}
		Ok(Self(value))
	}

	/// Return the exact validated model identifier.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl<'de> Deserialize<'de> for ConversationModel {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Closed Codex reasoning effort exposed by the Conversation controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationReasoningEffort {
	Low,
	Medium,
	High,
	XHigh,
	Max,
	Ultra,
}
impl ConversationReasoningEffort {
	/// Return the exact app-server wire value.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Low => "low",
			Self::Medium => "medium",
			Self::High => "high",
			Self::XHigh => "xhigh",
			Self::Max => "max",
			Self::Ultra => "ultra",
		}
	}
}

/// Explicit execution settings carried on every user send.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationExecutionSettings {
	pub model: ConversationModel,
	pub reasoning_effort: ConversationReasoningEffort,
	/// `true` maps to Codex's request-scoped `priority` service tier.
	pub fast: bool,
}
impl ConversationExecutionSettings {
	pub fn new(
		model: ConversationModel,
		reasoning_effort: ConversationReasoningEffort,
		fast: bool,
	) -> Self {
		Self { model, reasoning_effort, fast }
	}
}

/// Bounded normalized server-host path requested for one Conversation process.
///
/// This value grants no path authority. The daemon validates the path against its retained
/// descriptor and host policy immediately before child creation.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConversationWorkingDirectory(String);

impl ConversationWorkingDirectory {
	/// Validate one absolute lexical path without consulting client filesystem state.
	pub fn new(value: impl Into<String>) -> Result<Self, ConversationContractError> {
		let value = value.into();
		let components =
			value.strip_prefix('/').ok_or(ConversationContractError::InvalidWorkingDirectory)?;
		if components.is_empty()
			|| value.len() > MAX_CONVERSATION_WORKING_DIRECTORY_BYTES
			|| value.chars().any(char::is_control)
			|| components
				.split('/')
				.any(|component| component.is_empty() || matches!(component, "." | ".."))
		{
			return Err(ConversationContractError::InvalidWorkingDirectory);
		}

		Ok(Self(value))
	}

	/// Return the untrusted bounded host path input.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Debug for ConversationWorkingDirectory {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ConversationWorkingDirectory(<server-host-only>)")
	}
}

impl<'de> Deserialize<'de> for ConversationWorkingDirectory {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Minimal presentation state derived from ordinary durable facts and bounded local activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationState {
	/// The Conversation is durable but no L0 Routing Decision has committed yet.
	RoutingPending,
	/// A selected initial decision is durable, but first-session planning is incomplete.
	EstablishmentPending,
	/// The latest explicit L0 Routing Decision found only positive quota exhaustion.
	QuotaExhausted,
	/// The latest explicit L0 Routing Decision found no eligible account route.
	NoRoute,
	/// The initial ordinary RuntimeSession thread is being established.
	Establishing,
	/// The bound ordinary Conversation can accept another user Turn.
	Ready,
	/// One daemon-local turn handle is active.
	Running,
	/// Definite missing or incompatible authority requires an explicit action.
	ManualRecovery,
	/// A provider effect or command result may have occurred and must not be inferred or retried.
	OutcomeUnknown,
}

/// Definite terminal outcome for one ordinary provider-backed Turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationTurnOutcome {
	/// Positive provider evidence established successful completion.
	Succeeded,
	/// Positive provider evidence established definitive failure or interruption.
	Failed,
}

/// Closed user action for an ordinary Task conversation that cannot proceed automatically.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRecoveryAction {
	/// Resume the sole uncommitted initial route on this Conversation.
	ResumeRouting,
	/// Create one fresh routing Conversation successor for immutable waiting/no-route authority.
	CreateRoutingSuccessor,
	/// Resume first-session planning or establishment from committed durable coordinates.
	ResumeEstablishment,
	/// Configure deterministic account selection.
	ConfigureAccount,
	/// Enable the selected account.
	EnableAccount,
	/// Enroll account credentials.
	EnrollCredentials,
	/// Resolve an unsettled account operation.
	ResolveAccountOperation,
	/// Repair the protected credential store.
	RepairCredentialStore,
	/// Restore agreement between local and provider account identity.
	RestoreProviderAgreement,
	/// Refresh stale quota observations.
	RefreshQuota,
	/// Install the accepted Codex build.
	UpgradeCodex,
	/// Select a normalized effective-user-owned local working directory.
	SelectWorkingDirectory,
	/// Start a new ordinary Conversation because the existing thread cannot be resumed safely.
	StartNewConversation,
	/// Wait for or manually reconcile the previously active ordinary Turn.
	ResolvePriorActiveTurn,
	/// Reconcile the unresolved generic ProviderAttempt before submitting another Turn.
	ResolvePriorAttempt,
	/// Retry only after definite process readiness has been restored.
	RestoreProcessReadiness,
	/// Wait for the currently admitted daemon-local command or turn to settle.
	WaitForCurrentCommand,
	/// Refresh the ordinary Conversation readback after an exact authority conflict.
	RefreshConversation,
}

/// Positive bounded item count for one ordinary Task-conversation page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConversationListSize(u16);
impl ConversationListSize {
	/// Validate a public list size.
	pub const fn new(value: u16) -> Result<Self, ConversationContractError> {
		if value == 0 || value > MAX_CONVERSATION_LIST_SIZE {
			return Err(ConversationContractError::InvalidListSize);
		}
		Ok(Self(value))
	}

	/// Return the validated item count.
	pub const fn get(self) -> u16 {
		self.0
	}
}
impl<'de> Deserialize<'de> for ConversationListSize {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(u16::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Deterministic keyset position for ordinary Task-conversation listing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationListCursor {
	updated_at_micros: i64,
	conversation_id: EntityId,
}
impl ConversationListCursor {
	/// Validate one server-issued list position.
	pub fn new(
		updated_at_micros: i64,
		conversation_id: EntityId,
	) -> Result<Self, ConversationContractError> {
		if updated_at_micros <= 0 || !is_canonical_uuid_v4(conversation_id.as_str()) {
			return Err(ConversationContractError::InvalidCursor);
		}
		Ok(Self { updated_at_micros, conversation_id })
	}

	/// Exact last-seen activity timestamp in Unix microseconds.
	pub const fn updated_at_micros(&self) -> i64 {
		self.updated_at_micros
	}

	/// Exact last-seen ordinary Conversation identity.
	pub const fn conversation_id(&self) -> &EntityId {
		&self.conversation_id
	}
}
impl<'de> Deserialize<'de> for ConversationListCursor {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			updated_at_micros: i64,
			conversation_id: EntityId,
		}

		let raw = Raw::deserialize(deserializer)?;
		Self::new(raw.updated_at_micros, raw.conversation_id).map_err(D::Error::custom)
	}
}

/// Credential-negative projection of one ordinary Conversation and its sole current RuntimeSession.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSummary {
	/// Stable ordinary Conversation identity.
	pub conversation_id: EntityId,
	/// Meaningful durable title derived from persisted product authority.
	pub title: ConversationTitle,
	/// Exact provider thread identity, absent until authoritative app-server binding readback.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub codex_thread_id: Option<ProviderThreadId>,
	/// Exact Program WorkItem binding when this Conversation executes Program work.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub program: Option<ConversationProgramContext>,
	/// Exact ordinary Conversation revision represented by this projection.
	pub conversation_revision: EntityRevision,
	/// Product-store monotonic order of the durable facts represented by this projection.
	pub projection_updated_at_micros: i64,
	/// Sole current ordinary RuntimeSession identity, absent before first-session planning
	/// succeeds.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub runtime_session_id: Option<EntityId>,
	/// Exact RuntimeSession revision, jointly absent before first-session planning succeeds.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub runtime_session_revision: Option<EntityRevision>,
	/// Minimal durable-plus-local presentation state.
	pub state: ConversationState,
	/// Exact active logical user Turn, only while a local handle or durable recovery fact exists.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub active_turn_id: Option<EntityId>,
	/// Closed recovery action when `state` is `manual_recovery`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub recovery_action: Option<ConversationRecoveryAction>,
}
impl ConversationSummary {
	/// Validate one ordinary credential-negative projection.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		conversation_id: EntityId,
		title: ConversationTitle,
		codex_thread_id: Option<ProviderThreadId>,
		program: Option<ConversationProgramContext>,
		conversation_revision: EntityRevision,
		projection_updated_at_micros: i64,
		runtime_session_id: Option<EntityId>,
		runtime_session_revision: Option<EntityRevision>,
		state: ConversationState,
		active_turn_id: Option<EntityId>,
		recovery_action: Option<ConversationRecoveryAction>,
	) -> Result<Self, ConversationContractError> {
		let canonical = is_canonical_uuid_v4(conversation_id.as_str())
			&& runtime_session_id.as_ref().is_none_or(|id| is_canonical_uuid_v4(id.as_str()))
			&& active_turn_id.as_ref().is_none_or(|id| is_canonical_uuid_v4(id.as_str()));
		let has_session = runtime_session_id.is_some() && runtime_session_revision.is_some();
		let pre_session = matches!(
			state,
			ConversationState::RoutingPending
				| ConversationState::EstablishmentPending
				| ConversationState::QuotaExhausted
				| ConversationState::NoRoute
		);
		let state_shape = match state {
			ConversationState::RoutingPending =>
				active_turn_id.is_none()
					&& recovery_action == Some(ConversationRecoveryAction::ResumeRouting),
			ConversationState::EstablishmentPending =>
				active_turn_id.is_none()
					&& recovery_action == Some(ConversationRecoveryAction::ResumeEstablishment),
			ConversationState::QuotaExhausted | ConversationState::NoRoute =>
				active_turn_id.is_none()
					&& recovery_action == Some(ConversationRecoveryAction::CreateRoutingSuccessor),
			ConversationState::Establishing =>
				recovery_action.is_none()
					|| recovery_action == Some(ConversationRecoveryAction::ResumeEstablishment),
			ConversationState::Ready => active_turn_id.is_none() && recovery_action.is_none(),
			ConversationState::Running => active_turn_id.is_some() && recovery_action.is_none(),
			ConversationState::ManualRecovery => recovery_action.is_some(),
			ConversationState::OutcomeUnknown => recovery_action.is_none(),
		};
		if !canonical
			|| conversation_revision.0 == 0
			|| projection_updated_at_micros <= 0
			|| runtime_session_revision.as_ref().is_some_and(|revision| revision.0 == 0)
			|| has_session == pre_session
			|| runtime_session_id.is_some() != runtime_session_revision.is_some()
			|| codex_thread_id.is_some() && runtime_session_id.is_none()
			|| program.as_ref().is_some_and(|context| context.title != title)
			|| !state_shape
		{
			return Err(ConversationContractError::InvalidProjection);
		}
		Ok(Self {
			conversation_id,
			title,
			codex_thread_id,
			program,
			conversation_revision,
			projection_updated_at_micros,
			runtime_session_id,
			runtime_session_revision,
			state,
			active_turn_id,
			recovery_action,
		})
	}
}
impl<'de> Deserialize<'de> for ConversationSummary {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			conversation_id: EntityId,
			title: ConversationTitle,
			codex_thread_id: Option<ProviderThreadId>,
			program: Option<ConversationProgramContext>,
			conversation_revision: EntityRevision,
			projection_updated_at_micros: i64,
			runtime_session_id: Option<EntityId>,
			runtime_session_revision: Option<EntityRevision>,
			state: ConversationState,
			active_turn_id: Option<EntityId>,
			recovery_action: Option<ConversationRecoveryAction>,
		}

		let raw = Raw::deserialize(deserializer)?;
		Self::new(
			raw.conversation_id,
			raw.title,
			raw.codex_thread_id,
			raw.program,
			raw.conversation_revision,
			raw.projection_updated_at_micros,
			raw.runtime_session_id,
			raw.runtime_session_revision,
			raw.state,
			raw.active_turn_id,
			raw.recovery_action,
		)
		.map_err(D::Error::custom)
	}
}

/// One bounded deterministic page of ordinary Task conversations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationListPage {
	/// Most-recent-first ordinary Conversation projections.
	pub conversations: Vec<ConversationSummary>,
	/// Position for the next page, only when more rows exist.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub next_cursor: Option<ConversationListCursor>,
}
impl ConversationListPage {
	/// Validate the public page bound and unique Conversation identities.
	pub fn new(
		conversations: Vec<ConversationSummary>,
		next_cursor: Option<ConversationListCursor>,
	) -> Result<Self, ConversationContractError> {
		let mut identities = HashSet::with_capacity(conversations.len());
		if conversations.len() > usize::from(MAX_CONVERSATION_LIST_SIZE)
			|| conversations
				.iter()
				.any(|conversation| !identities.insert(conversation.conversation_id.as_str()))
		{
			return Err(ConversationContractError::InvalidProjection);
		}
		Ok(Self { conversations, next_cursor })
	}
}
impl<'de> Deserialize<'de> for ConversationListPage {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			conversations: Vec<ConversationSummary>,
			next_cursor: Option<ConversationListCursor>,
		}

		let raw = Raw::deserialize(deserializer)?;
		Self::new(raw.conversations, raw.next_cursor).map_err(D::Error::custom)
	}
}

/// Closed failure from an ordinary Task-conversation observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationReadError {
	/// The query shape or cursor was invalid.
	InvalidRequest,
	/// Persisted ordinary Conversation authority failed strict integrity checks.
	IntegrityUnavailable,
	/// Durable product state was unavailable.
	ProductStateUnavailable,
}

/// Bounded list result for the Conversations destination.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum ConversationListResult {
	/// One bounded deterministic page.
	Available(ConversationListPage),
	/// The ordinary read authority could not produce a safe projection.
	Unavailable {
		/// Closed reason that the list projection is unavailable.
		error: ConversationReadError,
	},
}

/// Readback for one exact ordinary Task conversation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // Available is the canonical bounded projection; other cases stay scalar.
pub enum ConversationResult {
	/// Current ordinary Conversation and RuntimeSession projection.
	Available(ConversationSummary),
	/// The requested archived source redirects to its sole routing successor.
	RoutingSuccessorRedirect {
		/// Archived source Conversation identity.
		source_conversation_id: EntityId,
		/// Exact archived source revision.
		source_conversation_revision: EntityRevision,
		/// Direct open successor Conversation identity.
		successor_conversation_id: EntityId,
		/// Exact current successor revision returned by the Conversation owner.
		successor_conversation_revision: EntityRevision,
	},
	/// No eligible ordinary Task conversation exists for the requested identity.
	NotFound,
	/// The ordinary read authority could not produce a safe projection.
	Unavailable {
		/// Closed reason that the exact projection is unavailable.
		error: ConversationReadError,
	},
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();
	bytes.len() == 36
		&& [8, 13, 18, 23].into_iter().all(|index| bytes[index] == b'-')
		&& bytes[14] == b'4'
		&& matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
		&& bytes.iter().enumerate().all(|(index, byte)| {
			[8, 13, 18, 23].contains(&index)
				|| byte.is_ascii_digit()
				|| (b'a'..=b'f').contains(byte)
		})
}

#[cfg(test)]
mod provider_thread_tests {
	use super::{MAX_PROVIDER_THREAD_ID_BYTES, ProviderThreadId};

	#[test]
	fn exact_persistence_boundary_is_512_utf8_bytes() {
		assert!(ProviderThreadId::new("x".repeat(MAX_PROVIDER_THREAD_ID_BYTES)).is_ok());
		assert!(ProviderThreadId::new("x".repeat(MAX_PROVIDER_THREAD_ID_BYTES + 1)).is_err());
	}

	#[test]
	fn codex_url_round_trips_one_encoded_path_segment_without_splitting() {
		for identity in [
			"opaque-thread-1",
			"slash/value",
			"question?value",
			"fragment#value",
			"percent%value",
			"space value",
			"Unicode-线程-🧵",
		] {
			let identity = ProviderThreadId::new(identity).expect("provider thread identity");
			let url = identity.codex_url().expect("canonical Codex URL");
			assert!(url.query().is_none());
			assert!(url.fragment().is_none());
			assert_eq!(url.path_segments().expect("hierarchical URL").count(), 1);
			assert_eq!(
				ProviderThreadId::from_codex_url(&url).expect("exact URL readback"),
				identity
			);
		}
	}
}
