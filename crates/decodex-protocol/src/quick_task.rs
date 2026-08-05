//! Bounded presentation facts for ordinary Task-role conversations.

use std::{
	collections::HashSet,
	fmt::{Debug, Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{EntityId, EntityRevision};

/// Maximum ordinary Task conversations returned by one list observation.
pub const MAX_QUICK_TASK_LIST_SIZE: u16 = 64;
/// Maximum UTF-8 bytes in one server-host Quick Task working-directory input.
pub const MAX_QUICK_TASK_WORKING_DIRECTORY_BYTES: usize = 4_096;

/// Closed, redacted reason that Quick Task execution was unavailable at daemon startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickTaskUnavailableReason {
	/// Exact PostgreSQL product-state authority was unavailable.
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
	/// This host platform cannot execute the accepted Quick Task process contract.
	UnsupportedPlatform,
}

/// Invalid bounded Quick Task presentation input or projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskContractError {
	/// A working-directory input was not a bounded normalized absolute path.
	InvalidWorkingDirectory,
	/// A requested list size was zero or exceeded the public bound.
	InvalidListSize,
	/// A list cursor was not a positive canonical ordinary Conversation position.
	InvalidCursor,
	/// An ordinary Conversation or RuntimeSession projection was internally inconsistent.
	InvalidProjection,
}
impl Display for QuickTaskContractError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidWorkingDirectory => "invalid Quick Task working directory",
			Self::InvalidListSize => "invalid Quick Task list size",
			Self::InvalidCursor => "invalid Quick Task list cursor",
			Self::InvalidProjection => "invalid Quick Task conversation projection",
		})
	}
}

/// Bounded normalized server-host path requested for one Quick Task process.
///
/// This value grants no path authority. The daemon validates the path against its retained
/// descriptor and host policy immediately before child creation.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct QuickTaskWorkingDirectory(String);

impl QuickTaskWorkingDirectory {
	/// Validate one absolute lexical path without consulting client filesystem state.
	pub fn new(value: impl Into<String>) -> Result<Self, QuickTaskContractError> {
		let value = value.into();
		let components =
			value.strip_prefix('/').ok_or(QuickTaskContractError::InvalidWorkingDirectory)?;
		if components.is_empty()
			|| value.len() > MAX_QUICK_TASK_WORKING_DIRECTORY_BYTES
			|| value.chars().any(char::is_control)
			|| components
				.split('/')
				.any(|component| component.is_empty() || matches!(component, "." | ".."))
		{
			return Err(QuickTaskContractError::InvalidWorkingDirectory);
		}

		Ok(Self(value))
	}

	/// Return the untrusted bounded host path input.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Debug for QuickTaskWorkingDirectory {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("QuickTaskWorkingDirectory(<server-host-only>)")
	}
}

impl<'de> Deserialize<'de> for QuickTaskWorkingDirectory {
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
pub enum QuickTaskState {
	/// The Conversation is durable but no L0 Routing Decision has committed yet.
	RoutingPending,
	/// The latest explicit L0 Routing Decision found only positive quota exhaustion.
	QuotaExhausted,
	/// The latest explicit L0 Routing Decision retained unresolved execution authority.
	WaitingReconciliation,
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
pub enum QuickTaskTurnOutcome {
	/// Positive provider evidence established successful completion.
	Succeeded,
	/// Positive provider evidence established definitive failure or interruption.
	Failed,
}

/// Closed user action for an ordinary Task conversation that cannot proceed automatically.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickTaskRecoveryAction {
	/// Explicitly request a new L0 Routing Decision for this pre-session Conversation.
	RetryRouting,
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
pub struct QuickTaskListSize(u16);
impl QuickTaskListSize {
	/// Validate a public list size.
	pub const fn new(value: u16) -> Result<Self, QuickTaskContractError> {
		if value == 0 || value > MAX_QUICK_TASK_LIST_SIZE {
			return Err(QuickTaskContractError::InvalidListSize);
		}
		Ok(Self(value))
	}

	/// Return the validated item count.
	pub const fn get(self) -> u16 {
		self.0
	}
}
impl<'de> Deserialize<'de> for QuickTaskListSize {
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
pub struct QuickTaskListCursor {
	updated_at_micros: i64,
	conversation_id: EntityId,
}
impl QuickTaskListCursor {
	/// Validate one server-issued list position.
	pub fn new(
		updated_at_micros: i64,
		conversation_id: EntityId,
	) -> Result<Self, QuickTaskContractError> {
		if updated_at_micros <= 0 || !is_canonical_uuid_v4(conversation_id.as_str()) {
			return Err(QuickTaskContractError::InvalidCursor);
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
impl<'de> Deserialize<'de> for QuickTaskListCursor {
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
pub struct QuickTaskSummary {
	/// Stable ordinary Conversation identity.
	pub conversation_id: EntityId,
	/// Exact ordinary Conversation revision represented by this projection.
	pub conversation_revision: EntityRevision,
	/// Sole current ordinary RuntimeSession identity, absent before first-session planning
	/// succeeds.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub runtime_session_id: Option<EntityId>,
	/// Exact RuntimeSession revision, jointly absent before first-session planning succeeds.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub runtime_session_revision: Option<EntityRevision>,
	/// Minimal durable-plus-local presentation state.
	pub state: QuickTaskState,
	/// Exact active logical user Turn, only while a local handle or durable recovery fact exists.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub active_turn_id: Option<EntityId>,
	/// Closed recovery action when `state` is `manual_recovery`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub recovery_action: Option<QuickTaskRecoveryAction>,
}
impl QuickTaskSummary {
	/// Validate one ordinary credential-negative projection.
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		conversation_id: EntityId,
		conversation_revision: EntityRevision,
		runtime_session_id: Option<EntityId>,
		runtime_session_revision: Option<EntityRevision>,
		state: QuickTaskState,
		active_turn_id: Option<EntityId>,
		recovery_action: Option<QuickTaskRecoveryAction>,
	) -> Result<Self, QuickTaskContractError> {
		let canonical = is_canonical_uuid_v4(conversation_id.as_str())
			&& runtime_session_id.as_ref().is_none_or(|id| is_canonical_uuid_v4(id.as_str()))
			&& active_turn_id.as_ref().is_none_or(|id| is_canonical_uuid_v4(id.as_str()));
		let has_session = runtime_session_id.is_some() && runtime_session_revision.is_some();
		let pre_session = matches!(
			state,
			QuickTaskState::RoutingPending
				| QuickTaskState::QuotaExhausted
				| QuickTaskState::WaitingReconciliation
				| QuickTaskState::NoRoute
		);
		let state_shape = match state {
			QuickTaskState::RoutingPending
			| QuickTaskState::QuotaExhausted
			| QuickTaskState::WaitingReconciliation
			| QuickTaskState::NoRoute =>
				active_turn_id.is_none()
					&& recovery_action == Some(QuickTaskRecoveryAction::RetryRouting),
			QuickTaskState::Establishing | QuickTaskState::Ready =>
				active_turn_id.is_none() && recovery_action.is_none(),
			QuickTaskState::Running => active_turn_id.is_some() && recovery_action.is_none(),
			QuickTaskState::ManualRecovery => recovery_action.is_some(),
			QuickTaskState::OutcomeUnknown => recovery_action.is_none(),
		};
		if !canonical
			|| conversation_revision.0 == 0
			|| runtime_session_revision.as_ref().is_some_and(|revision| revision.0 == 0)
			|| has_session == pre_session
			|| runtime_session_id.is_some() != runtime_session_revision.is_some()
			|| !state_shape
		{
			return Err(QuickTaskContractError::InvalidProjection);
		}
		Ok(Self {
			conversation_id,
			conversation_revision,
			runtime_session_id,
			runtime_session_revision,
			state,
			active_turn_id,
			recovery_action,
		})
	}
}
impl<'de> Deserialize<'de> for QuickTaskSummary {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			conversation_id: EntityId,
			conversation_revision: EntityRevision,
			runtime_session_id: Option<EntityId>,
			runtime_session_revision: Option<EntityRevision>,
			state: QuickTaskState,
			active_turn_id: Option<EntityId>,
			recovery_action: Option<QuickTaskRecoveryAction>,
		}

		let raw = Raw::deserialize(deserializer)?;
		Self::new(
			raw.conversation_id,
			raw.conversation_revision,
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
pub struct QuickTaskListPage {
	/// Most-recent-first ordinary Conversation projections.
	pub conversations: Vec<QuickTaskSummary>,
	/// Position for the next page, only when more rows exist.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub next_cursor: Option<QuickTaskListCursor>,
}
impl QuickTaskListPage {
	/// Validate the public page bound and unique Conversation identities.
	pub fn new(
		conversations: Vec<QuickTaskSummary>,
		next_cursor: Option<QuickTaskListCursor>,
	) -> Result<Self, QuickTaskContractError> {
		let mut identities = HashSet::with_capacity(conversations.len());
		if conversations.len() > usize::from(MAX_QUICK_TASK_LIST_SIZE)
			|| conversations
				.iter()
				.any(|conversation| !identities.insert(conversation.conversation_id.as_str()))
		{
			return Err(QuickTaskContractError::InvalidProjection);
		}
		Ok(Self { conversations, next_cursor })
	}
}
impl<'de> Deserialize<'de> for QuickTaskListPage {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Raw {
			conversations: Vec<QuickTaskSummary>,
			next_cursor: Option<QuickTaskListCursor>,
		}

		let raw = Raw::deserialize(deserializer)?;
		Self::new(raw.conversations, raw.next_cursor).map_err(D::Error::custom)
	}
}

/// Closed failure from an ordinary Task-conversation observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickTaskReadError {
	/// The query shape or cursor was invalid.
	InvalidRequest,
	/// Persisted ordinary Conversation authority failed strict integrity checks.
	IntegrityUnavailable,
	/// PostgreSQL product state was unavailable.
	ProductStateUnavailable,
}

/// Bounded list result for the Quick Tasks destination.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum QuickTaskListResult {
	/// One bounded deterministic page.
	Available(QuickTaskListPage),
	/// The ordinary read authority could not produce a safe projection.
	Unavailable {
		/// Closed reason that the list projection is unavailable.
		error: QuickTaskReadError,
	},
}

/// Readback for one exact ordinary Task conversation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum QuickTaskResult {
	/// Current ordinary Conversation and RuntimeSession projection.
	Available(QuickTaskSummary),
	/// No eligible ordinary Task conversation exists for the requested identity.
	NotFound,
	/// The ordinary read authority could not produce a safe projection.
	Unavailable {
		/// Closed reason that the exact projection is unavailable.
		error: QuickTaskReadError,
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
