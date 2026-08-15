//! Ordinary Quick Task conversations, turns, and normalized history.

use decodex_core::{
	AccountId, ArtifactId, BlobHash, BlobStore, ConversationId, HistoryItemId, HistoryItemKind,
	HistoryMediaType, HistoryMetadata, ItemStatus, MAX_BLOB_BYTES, MAX_CONTEXT_RECENT_ITEMS,
	MAX_INLINE_HISTORY_BYTES, PossibleSideEffects, ProcessGenerationId, ProviderAttemptId,
	ProviderEvidenceId, ProviderRequestId, ProviderRequestKey, ProviderTerminalOutcome,
	RuntimeSessionId, RuntimeSessionState, TurnId, TurnRole, TurnStatus, WorkItemId,
};
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
	CommandIdentity, SqliteStore, StoreError,
	account_lifecycle::{random_uuid_v4, sql_error},
	unix_micros,
};

const MAX_PAGE_SIZE: u16 = 100;
const MAX_RECOVERED_ASSISTANT_BYTES: usize = 256 * 1_024;

/// Create one ordinary Quick Task conversation and retain its original request.
#[derive(Clone, Debug)]
pub struct CreateQuickTaskConversation {
	pub conversation_id: ConversationId,
	pub work_item_id: Option<WorkItemId>,
	pub title: String,
	pub message: String,
	pub working_directory: String,
	pub model: String,
	pub reasoning_effort: String,
	pub fast: bool,
}

/// Immutable original request coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskRequest {
	pub message: String,
	pub working_directory: String,
	pub model: String,
	pub reasoning_effort: String,
	pub fast: bool,
}

/// Exact active projection that may be closed after provider archive verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveQuickTaskConversation {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
}

/// Durable archived projection returned by the atomic local close.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArchivedQuickTaskConversation {
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveQuickTaskConversationOutcome {
	Applied(ArchivedQuickTaskConversation),
	Replayed(ArchivedQuickTaskConversation),
	Rejected,
}

/// Exact provider-less starting projection that can be closed without a Codex mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveLocalQuickTaskConversation {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveLocalQuickTaskConversationOutcome {
	Applied(ArchivedQuickTaskConversation),
	Replayed(ArchivedQuickTaskConversation),
	Rejected,
}

/// Exact inactive-owner coordinates for one admitted turn with no possible provider effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileStrandedQuickTaskTurn {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
	pub turn_id: TurnId,
	pub expected_turn_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconcileStrandedQuickTaskTurnOutcome {
	Applied { turn_revision: i64 },
	Replayed { turn_revision: i64 },
	Rejected,
}

/// Exact active unknown provider attempt that may be reconciled without replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownQuickTaskAttemptReadback {
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub runtime_session_revision: i64,
	pub codex_thread_id: String,
	pub source_account_id: AccountId,
	pub source_account_revision: i64,
	pub user_turn_id: TurnId,
	pub user_turn_revision: i64,
	pub user_turn_sequence: i64,
	pub attempt_id: ProviderAttemptId,
	pub attempt_revision: i64,
	pub request_id: ProviderRequestId,
	pub provider_key: ProviderRequestKey,
	pub process_generation_id: ProcessGenerationId,
	pub process_generation_is_dead: bool,
}

/// Durable positive evidence that still needs the Conversation Turn terminalization transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingQuickTaskTerminalizationReadback {
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub runtime_session_revision: i64,
	pub codex_thread_id: String,
	pub user_turn_id: TurnId,
	pub user_turn_revision: i64,
	pub user_turn_sequence: i64,
	pub attempt_id: ProviderAttemptId,
	pub attempt_revision: i64,
	pub evidence_id: ProviderEvidenceId,
	pub provider_outcome: ProviderTerminalOutcome,
	pub provider_turn_id: String,
}

/// Close one product-visible Turn after exact process death, while retaining its unknown attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoverUnknownQuickTaskTurn {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
	pub user_turn_id: TurnId,
	pub expected_user_turn_revision: i64,
	pub attempt_id: ProviderAttemptId,
	pub expected_attempt_revision: i64,
	pub process_generation_id: ProcessGenerationId,
	pub history_item_id: HistoryItemId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveredUnknownQuickTaskTurn {
	pub turn_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoverUnknownQuickTaskTurnOutcome {
	Applied(RecoveredUnknownQuickTaskTurn),
	Replayed(RecoveredUnknownQuickTaskTurn),
	Rejected,
}

/// Existing assistant prefix for one interrupted active user Turn.
#[derive(Clone, Eq, PartialEq)]
pub struct QuickTaskAssistantPrefixReadback {
	pub turn_id: TurnId,
	pub turn_revision: i64,
	pub turn_sequence: i64,
	pub text: String,
	pub next_ordinal: i32,
}

impl std::fmt::Debug for QuickTaskAssistantPrefixReadback {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("QuickTaskAssistantPrefixReadback")
			.field("turn_id", &self.turn_id)
			.field("turn_revision", &self.turn_revision)
			.field("turn_sequence", &self.turn_sequence)
			.field("text_bytes", &self.text.len())
			.field("next_ordinal", &self.next_ordinal)
			.finish()
	}
}

/// Create a new routing conversation after a waiting or no-route decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateQuickTaskRoutingSuccessor {
	pub source_conversation_id: ConversationId,
	pub expected_source_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QuickTaskRoutingSuccessor {
	pub source_conversation_id: ConversationId,
	pub source_revision: i64,
	pub successor_conversation_id: ConversationId,
	pub successor_revision: i64,
	pub source_routing_decision_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskRoutingSuccessorOutcome {
	Fresh(QuickTaskRoutingSuccessor),
	Replayed(QuickTaskRoutingSuccessor),
	Rejected { code: String, replayed: bool },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredConversation {
	pub conversation_id: ConversationId,
	pub title: String,
	pub revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TurnReservationReadback {
	pub turn_id: TurnId,
	pub sequence: i64,
	pub status: TurnStatus,
	pub revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnReservationOutcome {
	Fresh(TurnReservationReadback),
	Replayed(TurnReservationReadback),
}

#[derive(Clone, Debug)]
pub struct AdmitInitialQuickTaskTurn {
	pub expected_conversation_revision: i64,
	pub expected_runtime_session_revision: i64,
	pub continuation_plan_id: String,
	pub message: RecordHistoryItem,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InitialQuickTaskTurnAdmissionReadback {
	pub routing_decision_id: String,
	pub continuation_plan_id: String,
	pub turn: TurnReservationReadback,
	pub history_item_id: HistoryItemId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialQuickTaskTurnAdmissionRejection {
	InvalidInput,
	AuthorityUnavailable,
	InitialAdmissionConflict,
	MessageBlobMissing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitialQuickTaskTurnAdmissionOutcome {
	Fresh(InitialQuickTaskTurnAdmissionReadback),
	Replayed(InitialQuickTaskTurnAdmissionReadback),
	Rejected { rejection: InitialQuickTaskTurnAdmissionRejection, replayed: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalizeQuickTaskTurn {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
	pub user_turn_id: TurnId,
	pub expected_user_turn_revision: i64,
	pub assistant_turn: Option<(TurnId, i64)>,
	pub provider_attempt_id: ProviderAttemptId,
	pub expected_provider_attempt_revision: i64,
	pub provider_evidence_id: ProviderEvidenceId,
	pub provider_outcome: ProviderTerminalOutcome,
	pub provider_thread_id: String,
	pub provider_turn_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QuickTaskTerminalizationReadback {
	pub runtime_session_revision: i64,
	pub user_turn_revision: i64,
	pub assistant_turn_revision: Option<i64>,
	pub provider_attempt_revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskTerminalizationOutcome {
	Applied(QuickTaskTerminalizationReadback),
	Replayed(QuickTaskTerminalizationReadback),
	Rejected,
	Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryTaskConversationCursor {
	pub updated_at_micros: i64,
	pub conversation_id: ConversationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryTaskConversationReadback {
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
	pub runtime_session_id: Option<RuntimeSessionId>,
	pub runtime_session_revision: Option<i64>,
	pub runtime_session_state: Option<RuntimeSessionState>,
	pub has_acknowledged_turn: bool,
	pub active_turn_id: Option<TurnId>,
	pub active_turn_revision: Option<i64>,
	pub has_admitted_user_turn: bool,
	pub has_active_provider_attempt: bool,
	pub has_unknown_provider_attempt: bool,
	pub pre_session_state: Option<OrdinaryTaskPreSessionState>,
	pub routing_decision_id: Option<String>,
	pub updated_at_micros: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrdinaryTaskConversationProjection {
	Current(OrdinaryTaskConversationReadback),
	Archived {
		conversation_id: ConversationId,
		conversation_revision: i64,
	},
	RoutingSuccessorRedirect {
		source_conversation_id: ConversationId,
		source_revision: i64,
		successor_conversation_id: ConversationId,
		successor_conversation_revision: i64,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryTaskPreSessionState {
	RoutingPending,
	EstablishmentPending,
	QuotaExhausted,
	NoRoute,
}

/// One normalized history item mutation.
#[derive(Clone, Debug)]
pub struct RecordHistoryItem {
	pub conversation_id: ConversationId,
	pub runtime_session_id: RuntimeSessionId,
	pub turn_id: TurnId,
	pub turn_sequence: i64,
	pub turn_role: TurnRole,
	pub possible_side_effects: PossibleSideEffects,
	pub history_item_id: HistoryItemId,
	pub ordinal: i32,
	pub kind: HistoryItemKind,
	pub status: ItemStatus,
	pub text: String,
	pub media_type: HistoryMediaType,
	pub metadata: HistoryMetadata,
	pub expected_revision: Option<i64>,
	pub artifact: Option<(ArtifactId, i64)>,
}

/// Opaque position in one conversation history stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryCursor {
	sequence: i64,
}

impl HistoryCursor {
	pub fn encode(&self) -> String {
		format!("v1:{}", self.sequence)
	}

	pub fn parse(value: &str) -> Result<Self, StoreError> {
		let sequence = value
			.strip_prefix("v1:")
			.and_then(|value| value.parse::<i64>().ok())
			.filter(|value| *value > 0)
			.ok_or(StoreError::InvalidInput("history cursor is malformed"))?;
		Ok(Self { sequence })
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
	pub history_item_id: String,
	pub turn_id: String,
	pub runtime_session_id: String,
	pub turn_role: TurnRole,
	pub possible_side_effects: PossibleSideEffects,
	pub kind: HistoryItemKind,
	pub status: ItemStatus,
	pub inline_text: Option<String>,
	pub blob_hash: Option<BlobHash>,
	pub blob_byte_length: Option<u64>,
	pub media_type: HistoryMediaType,
	pub metadata: HistoryMetadata,
	pub artifact: Option<(ArtifactId, u64)>,
	pub revision: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryPage {
	pub entries: Vec<HistoryEntry>,
	pub next_cursor: Option<HistoryCursor>,
}

#[derive(Serialize, Deserialize)]
struct HistoryReceipt {
	history_item_id: String,
}

#[derive(Clone)]
struct Payload {
	inline_text: Option<String>,
	blob_hash: Option<String>,
}

impl SqliteStore {
	pub async fn create_quick_task_conversation(
		&self,
		command: &CommandIdentity,
		create: &CreateQuickTaskConversation,
	) -> Result<StoredConversation, StoreError> {
		validate_quick_task_conversation(create)?;
		let command = command.clone();
		let create = create.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				"create_quick_task_conversation",
				create.conversation_id.as_str(),
			)? {
				let stored: StoredConversation = serde_json::from_str(&response)
					.map_err(|_| incompatible("Conversation receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(stored);
			}
			let exists: bool = transaction
				.query_row(
					"SELECT EXISTS (SELECT 1 FROM conversations WHERE conversation_id = ?1)",
					params![create.conversation_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if exists {
				return Err(StoreError::RevisionConflict {
					entity: format!("conversation/{}", create.conversation_id),
					expected: None,
					actual: Some(1),
				});
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let initial_turn_id = random_uuid_v4()?;
			transaction
				.execute(
					"INSERT INTO conversations (
				   conversation_id, kind, state, title, revision, created_at_micros, updated_at_micros
				 ) VALUES (?1, 'ordinary_task', 'active', ?2, 1, ?3, ?3)",
					params![create.conversation_id.as_str(), create.title, now],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO quick_task_requests (
				   conversation_id, operation_key, correlation_id, initial_turn_id,
					 message, working_directory, model, reasoning_effort, fast, created_at_micros
				 ) VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
					params![
						create.conversation_id.as_str(),
						command.key,
						initial_turn_id,
						create.message,
						create.working_directory,
						create.model,
						create.reasoning_effort,
						create.fast,
						now,
					],
				)
				.map_err(sql_error)?;
			if let Some(work_item_id) = create.work_item_id.as_ref() {
				crate::program_cycles::bind_program_work_item_execution(
					&transaction,
					work_item_id,
					&create.conversation_id,
					now,
				)?;
			}
			let stored = StoredConversation {
				conversation_id: create.conversation_id,
				title: create.title,
				revision: 1,
			};
			write_receipt(
				&transaction,
				&command,
				"create_quick_task_conversation",
				stored.conversation_id.as_str(),
				&serde_json::to_string(&stored)
					.map_err(|_| incompatible("Conversation receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(stored)
		})
		.await
	}

	pub async fn read_quick_task_request(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<QuickTaskRequest>, StoreError> {
		let conversation_id = conversation_id.clone();
		self.run(move |connection| {
			connection
				.query_row(
					"SELECT q.message, q.working_directory, q.model, q.reasoning_effort, q.fast
				 FROM quick_task_requests AS q
				 JOIN conversations AS c USING (conversation_id)
				 WHERE q.conversation_id = ?1 AND c.state = 'active'",
					params![conversation_id.as_str()],
					|row| {
						Ok(QuickTaskRequest {
							message: row.get(0)?,
							working_directory: row.get(1)?,
							model: row.get(2)?,
							reasoning_effort: row.get(3)?,
							fast: row.get(4)?,
						})
					},
				)
				.optional()
				.map_err(sql_error)
		})
		.await
	}

	/// Read one exact active unknown attempt for account-bound, no-replay reconciliation.
	pub async fn read_unknown_quick_task_attempt_for_recovery(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<UnknownQuickTaskAttemptReadback>, StoreError> {
		let conversation_id = conversation_id.clone();
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT c.revision, s.runtime_session_id, s.revision, s.codex_thread_id,
				        s.account_id, s.account_revision, t.turn_id, t.revision, t.sequence,
				        p.attempt_id, p.revision, p.request_id,
				        COALESCE(p.provider_idempotency_key, p.provider_correlation_key),
				        p.process_generation_id,
				        pg.state = 'dead' AND pg.death_evidence_id IS NOT NULL AND EXISTS (
				          SELECT 1 FROM process_generation_death_evidence AS d
				          WHERE d.generation_id = pg.generation_id
				            AND d.evidence_id = pg.death_evidence_id
				        )
				 FROM conversations AS c
				 JOIN runtime_sessions AS s ON s.conversation_id = c.conversation_id
				 JOIN turns AS t ON t.runtime_session_id = s.runtime_session_id
				 JOIN provider_attempts AS p ON p.turn_id = t.turn_id
				   AND p.runtime_session_id = s.runtime_session_id
				 JOIN process_generations AS pg ON pg.generation_id = p.process_generation_id
				 WHERE c.conversation_id = ?1 AND c.kind = 'ordinary_task' AND c.state = 'active'
				   AND s.state = 'active' AND s.codex_thread_id IS NOT NULL
				   AND t.role = 'user' AND t.status = 'active' AND p.state = 'unknown'
				 ORDER BY t.sequence DESC, p.created_at_micros DESC LIMIT 2",
				)
				.map_err(sql_error)?;
			let selected = statement
				.query_map(params![conversation_id.as_str()], |row| {
					Ok((
						row.get::<_, i64>(0)?,
						row.get::<_, String>(1)?,
						row.get::<_, i64>(2)?,
						row.get::<_, String>(3)?,
						row.get::<_, String>(4)?,
						row.get::<_, i64>(5)?,
						row.get::<_, String>(6)?,
						row.get::<_, i64>(7)?,
						row.get::<_, i64>(8)?,
						row.get::<_, String>(9)?,
						row.get::<_, i64>(10)?,
						row.get::<_, String>(11)?,
						row.get::<_, String>(12)?,
						row.get::<_, String>(13)?,
						row.get::<_, bool>(14)?,
					))
				})
				.map_err(sql_error)?;
			let mut rows = selected.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
			if rows.len() > 1 {
				return Err(incompatible("unknown Quick Task attempt authority"));
			}
			let Some(row) = rows.pop() else {
				return Ok(None);
			};
			if row.0 <= 0 || row.2 <= 0 || row.5 <= 0 || row.7 <= 0 || row.8 <= 0 || row.10 <= 0 {
				return Err(incompatible("unknown Quick Task attempt coordinates"));
			}
			Ok(Some(UnknownQuickTaskAttemptReadback {
				conversation_id,
				conversation_revision: row.0,
				runtime_session_id: RuntimeSessionId::new(row.1)
					.map_err(|_| incompatible("RuntimeSession identity"))?,
				runtime_session_revision: row.2,
				codex_thread_id: row.3,
				source_account_id: AccountId::new(row.4)
					.map_err(|_| incompatible("RuntimeSession account"))?,
				source_account_revision: row.5,
				user_turn_id: TurnId::new(row.6)
					.map_err(|_| incompatible("unknown Turn identity"))?,
				user_turn_revision: row.7,
				user_turn_sequence: row.8,
				attempt_id: ProviderAttemptId::new(row.9)
					.map_err(|_| incompatible("ProviderAttempt identity"))?,
				attempt_revision: row.10,
				request_id: ProviderRequestId::new(row.11)
					.map_err(|_| incompatible("provider request identity"))?,
				provider_key: ProviderRequestKey::new(row.12)
					.map_err(|_| incompatible("provider request key"))?,
				process_generation_id: ProcessGenerationId::new(row.13)
					.map_err(|_| incompatible("ProcessGeneration identity"))?,
				process_generation_is_dead: row.14,
			}))
		})
		.await
	}

	/// Read one exact positive provider result whose active Turn has not yet terminalized.
	pub async fn read_pending_quick_task_terminalization(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<PendingQuickTaskTerminalizationReadback>, StoreError> {
		let conversation_id = conversation_id.clone();
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT c.revision, s.runtime_session_id, s.revision, s.codex_thread_id,
				        t.turn_id, t.revision, t.sequence, p.attempt_id, p.revision,
				        e.evidence_id, e.outcome, e.provider_turn_id
				 FROM conversations AS c
				 JOIN runtime_sessions AS s ON s.conversation_id = c.conversation_id
				 JOIN turns AS t ON t.runtime_session_id = s.runtime_session_id
				 JOIN provider_attempts AS p ON p.turn_id = t.turn_id
				   AND p.runtime_session_id = s.runtime_session_id
				 JOIN provider_attempt_positive_evidence AS e ON e.attempt_id = p.attempt_id
				   AND e.evidence_id = p.terminal_evidence_id
				 WHERE c.conversation_id = ?1 AND c.kind = 'ordinary_task' AND c.state = 'active'
				   AND s.state = 'active' AND s.codex_thread_id IS NOT NULL
				   AND t.role = 'user' AND t.status = 'active'
				   AND p.state IN ('succeeded', 'failed_definitive') AND e.outcome = p.state
				   AND e.provider_thread_id = s.codex_thread_id AND e.provider_turn_id IS NOT NULL
				 ORDER BY t.sequence DESC, p.created_at_micros DESC LIMIT 2",
				)
				.map_err(sql_error)?;
			let selected = statement
				.query_map(params![conversation_id.as_str()], |row| {
					Ok((
						row.get::<_, i64>(0)?,
						row.get::<_, String>(1)?,
						row.get::<_, i64>(2)?,
						row.get::<_, String>(3)?,
						row.get::<_, String>(4)?,
						row.get::<_, i64>(5)?,
						row.get::<_, i64>(6)?,
						row.get::<_, String>(7)?,
						row.get::<_, i64>(8)?,
						row.get::<_, String>(9)?,
						row.get::<_, String>(10)?,
						row.get::<_, String>(11)?,
					))
				})
				.map_err(sql_error)?;
			let mut rows = selected.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
			if rows.len() > 1 {
				return Err(incompatible("pending Quick Task terminalization authority"));
			}
			let Some(row) = rows.pop() else {
				return Ok(None);
			};
			if row.0 <= 0 || row.2 <= 0 || row.5 <= 0 || row.6 <= 0 || row.8 <= 0 {
				return Err(incompatible("pending Quick Task terminalization coordinates"));
			}
			Ok(Some(PendingQuickTaskTerminalizationReadback {
				conversation_id,
				conversation_revision: row.0,
				runtime_session_id: RuntimeSessionId::new(row.1)
					.map_err(|_| incompatible("RuntimeSession identity"))?,
				runtime_session_revision: row.2,
				codex_thread_id: row.3,
				user_turn_id: TurnId::new(row.4)
					.map_err(|_| incompatible("terminal user Turn identity"))?,
				user_turn_revision: row.5,
				user_turn_sequence: row.6,
				attempt_id: ProviderAttemptId::new(row.7)
					.map_err(|_| incompatible("ProviderAttempt identity"))?,
				attempt_revision: row.8,
				evidence_id: ProviderEvidenceId::new(row.9)
					.map_err(|_| incompatible("provider evidence identity"))?,
				provider_outcome: parse_provider_outcome(&row.10)?,
				provider_turn_id: row.11,
			}))
		})
		.await
	}

	/// Read the exact durable assistant prefix already captured for one active user Turn.
	pub async fn read_quick_task_assistant_prefix(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		runtime_session_id: &RuntimeSessionId,
		user_turn_sequence: i64,
	) -> Result<Option<QuickTaskAssistantPrefixReadback>, StoreError> {
		if user_turn_sequence <= 0 {
			return Err(StoreError::InvalidInput("assistant prefix sequence is invalid"));
		}
		let assistant_sequence = user_turn_sequence
			.checked_add(1)
			.ok_or(StoreError::InvalidInput("assistant prefix sequence is invalid"))?;
		let conversation_id = conversation_id.clone();
		let runtime_session_id = runtime_session_id.clone();
		let row = self
			.run(move |connection| {
				let turn = connection
					.query_row(
						"SELECT turn_id, revision FROM turns
						 WHERE conversation_id = ?1 AND runtime_session_id = ?2 AND sequence = ?3
						   AND role = 'assistant' AND status = 'active'",
						params![
							conversation_id.as_str(),
							runtime_session_id.as_str(),
							assistant_sequence,
						],
						|row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
					)
					.optional()
					.map_err(sql_error)?;
				let Some((turn_id, turn_revision)) = turn else {
					return Ok(None);
				};
				let mut statement = connection
					.prepare(
						"SELECT kind, status, inline_text, blob_sha256 FROM history_items
						 WHERE conversation_id = ?1 AND turn_id = ?2
						 ORDER BY sequence LIMIT ?3",
					)
					.map_err(sql_error)?;
				let selected = statement
					.query_map(
						params![
							conversation_id.as_str(),
							turn_id,
							i64::try_from(MAX_CONTEXT_RECENT_ITEMS + 1).unwrap_or(i64::MAX),
						],
						|row| {
							Ok((
								row.get::<_, String>(0)?,
								row.get::<_, String>(1)?,
								Payload { inline_text: row.get(2)?, blob_hash: row.get(3)? },
							))
						},
					)
					.map_err(sql_error)?;
				let items = selected.collect::<Result<Vec<_>, _>>().map_err(sql_error)?;
				if items.is_empty()
					|| items.len() > MAX_CONTEXT_RECENT_ITEMS
					|| items.iter().any(|(kind, status, payload)| {
						kind != "message"
							|| status != "completed"
							|| payload.inline_text.is_some() == payload.blob_hash.is_some()
					}) {
					return Err(incompatible("assistant recovery prefix"));
				}
				Ok(Some((turn_id, turn_revision, items)))
			})
			.await?;
		let Some((turn_id, turn_revision, items)) = row else {
			return Ok(None);
		};
		let mut text = String::new();
		for (_, _, payload) in &items {
			let chunk = match (&payload.inline_text, &payload.blob_hash) {
				(Some(inline), None) => inline.clone(),
				(None, Some(hash)) => {
					let hash = BlobHash::parse(hash)
						.map_err(|_| incompatible("assistant recovery blob identity"))?;
					String::from_utf8(blob_store.read(hash)?)
						.map_err(|_| incompatible("assistant recovery blob encoding"))?
				},
				_ => return Err(incompatible("assistant recovery payload")),
			};
			text.len()
				.checked_add(chunk.len())
				.filter(|length| *length <= MAX_RECOVERED_ASSISTANT_BYTES)
				.ok_or_else(|| incompatible("assistant recovery prefix bound"))?;
			text.push_str(&chunk);
		}
		let next_ordinal =
			i32::try_from(items.len()).map_err(|_| incompatible("assistant recovery ordinal"))?;
		Ok(Some(QuickTaskAssistantPrefixReadback {
			turn_id: TurnId::new(turn_id).map_err(|_| incompatible("assistant Turn identity"))?,
			turn_revision,
			turn_sequence: assistant_sequence,
			text,
			next_ordinal,
		}))
	}

	/// Atomically close one provider-verified archived Conversation and its sole active session.
	pub async fn archive_quick_task_conversation(
		&self,
		command: &CommandIdentity,
		request: &ArchiveQuickTaskConversation,
	) -> Result<ArchiveQuickTaskConversationOutcome, StoreError> {
		if request.expected_conversation_revision <= 0
			|| request.expected_runtime_session_revision <= 0
		{
			return Err(StoreError::InvalidInput("Quick Task archive coordinates are invalid"));
		}
		let command = command.clone();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				"archive_quick_task_conversation",
				request.conversation_id.as_str(),
			)? {
				let archived = serde_json::from_str(&response)
					.map_err(|_| incompatible("Quick Task archive receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(ArchiveQuickTaskConversationOutcome::Replayed(archived));
			}
			let authority = transaction
				.query_row(
					"SELECT c.revision, s.revision,
					        EXISTS (SELECT 1 FROM turns AS t WHERE t.conversation_id = c.conversation_id
					                AND t.status = 'active'),
					        EXISTS (SELECT 1 FROM provider_attempts AS p
					                JOIN turns AS pending ON pending.turn_id = p.turn_id
					                WHERE p.conversation_id = c.conversation_id
					                  AND p.state IN ('prepared', 'dispatch_authorized', 'unknown')
					                  AND pending.status = 'active')
					 FROM conversations AS c
					 JOIN runtime_sessions AS s ON s.conversation_id = c.conversation_id
					 WHERE c.conversation_id = ?1 AND c.kind = 'ordinary_task' AND c.state = 'active'
					   AND s.runtime_session_id = ?2 AND s.state = 'active'",
					params![request.conversation_id.as_str(), request.runtime_session_id.as_str()],
					|row| {
						Ok((
							row.get::<_, i64>(0)?,
							row.get::<_, i64>(1)?,
							row.get::<_, bool>(2)?,
							row.get::<_, bool>(3)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some((conversation_revision, session_revision, active_turn, active_attempt)) =
				authority
			else {
				return Ok(ArchiveQuickTaskConversationOutcome::Rejected);
			};
			if conversation_revision != request.expected_conversation_revision
				|| session_revision != request.expected_runtime_session_revision
				|| active_turn
				|| active_attempt
			{
				return Ok(ArchiveQuickTaskConversationOutcome::Rejected);
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let session_changed = transaction
				.execute(
					"UPDATE runtime_sessions SET state = 'ended', revision = revision + 1,
					 updated_at_micros = ?3, ended_at_micros = ?3
					 WHERE runtime_session_id = ?1 AND revision = ?2 AND state = 'active'",
					params![
						request.runtime_session_id.as_str(),
						request.expected_runtime_session_revision,
						now,
					],
				)
				.map_err(sql_error)?;
			let conversation_changed = transaction
				.execute(
					"UPDATE conversations SET state = 'archived', revision = revision + 1,
					 updated_at_micros = ?3 WHERE conversation_id = ?1 AND revision = ?2
					 AND state = 'active'",
					params![
						request.conversation_id.as_str(),
						request.expected_conversation_revision,
						now,
					],
				)
				.map_err(sql_error)?;
			if session_changed != 1 || conversation_changed != 1 {
				return Ok(ArchiveQuickTaskConversationOutcome::Rejected);
			}
			let archived = ArchivedQuickTaskConversation {
				conversation_id: request.conversation_id,
				conversation_revision: request.expected_conversation_revision + 1,
			};
			write_receipt(
				&transaction,
				&command,
				"archive_quick_task_conversation",
				archived.conversation_id.as_str(),
				&serde_json::to_string(&archived)
					.map_err(|_| incompatible("Quick Task archive receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ArchiveQuickTaskConversationOutcome::Applied(archived))
		})
		.await
	}

	/// Fail one stale admitted user Turn only when durable authority proves no live process or
	/// provider attempt can still own an effect.
	pub async fn reconcile_stranded_quick_task_turn(
		&self,
		command: &CommandIdentity,
		request: &ReconcileStrandedQuickTaskTurn,
	) -> Result<ReconcileStrandedQuickTaskTurnOutcome, StoreError> {
		if request.expected_conversation_revision <= 0
			|| request.expected_runtime_session_revision <= 0
			|| request.expected_turn_revision <= 0
		{
			return Err(StoreError::InvalidInput(
				"stranded Quick Task Turn coordinates are invalid",
			));
		}
		let command = command.clone();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				"reconcile_stranded_quick_task_turn",
				request.turn_id.as_str(),
			)? {
				let turn_revision = response
					.parse::<i64>()
					.map_err(|_| incompatible("stranded Quick Task Turn receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(ReconcileStrandedQuickTaskTurnOutcome::Replayed { turn_revision });
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = transaction
				.execute(
					"UPDATE turns SET status = 'failed', revision = revision + 1,
					 updated_at_micros = ?7, completed_at_micros = ?7
					 WHERE turn_id = ?1 AND conversation_id = ?2 AND runtime_session_id = ?3
					   AND revision = ?6 AND role = 'user' AND status = 'active'
					   AND EXISTS (
					     SELECT 1 FROM conversations AS c JOIN runtime_sessions AS s
					       ON s.conversation_id = c.conversation_id
					     WHERE c.conversation_id = ?2 AND c.state = 'active' AND c.revision = ?4
					       AND s.runtime_session_id = ?3 AND s.state IN ('starting', 'active')
					       AND s.revision = ?5
					   )
					   AND NOT EXISTS (
					     SELECT 1 FROM provider_attempts AS p WHERE p.turn_id = ?1
					       AND p.state IN ('prepared', 'dispatch_authorized', 'unknown')
					   )
					   AND NOT EXISTS (
					     SELECT 1 FROM process_generations AS p
					     WHERE p.runtime_session_id = ?3 AND p.state <> 'dead'
					   )
					   AND NOT EXISTS (
					     SELECT 1 FROM history_items AS h
					     WHERE h.turn_id = ?1 AND h.status = 'streaming'
					   )",
					params![
						request.turn_id.as_str(),
						request.conversation_id.as_str(),
						request.runtime_session_id.as_str(),
						request.expected_conversation_revision,
						request.expected_runtime_session_revision,
						request.expected_turn_revision,
						now,
					],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Ok(ReconcileStrandedQuickTaskTurnOutcome::Rejected);
			}
			transaction
				.execute(
					"UPDATE conversations SET updated_at_micros = ?2
					 WHERE conversation_id = ?1 AND state = 'active' AND updated_at_micros <= ?2",
					params![request.conversation_id.as_str(), now],
				)
				.map_err(sql_error)?;
			let turn_revision = request.expected_turn_revision + 1;
			write_receipt(
				&transaction,
				&command,
				"reconcile_stranded_quick_task_turn",
				request.turn_id.as_str(),
				&turn_revision.to_string(),
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ReconcileStrandedQuickTaskTurnOutcome::Applied { turn_revision })
		})
		.await
	}

	/// Make an unknown Turn usable only after positive death of its exact process generation.
	///
	/// The ProviderAttempt intentionally remains `unknown`; this transaction only closes the
	/// product-visible active Turn and records a concise durable status item.
	pub async fn recover_unknown_quick_task_turn(
		&self,
		command: &CommandIdentity,
		request: &RecoverUnknownQuickTaskTurn,
	) -> Result<RecoverUnknownQuickTaskTurnOutcome, StoreError> {
		if request.expected_conversation_revision <= 0
			|| request.expected_runtime_session_revision <= 0
			|| request.expected_user_turn_revision <= 0
			|| request.expected_attempt_revision <= 0
		{
			return Err(StoreError::InvalidInput(
				"unknown Quick Task recovery coordinates are invalid",
			));
		}
		let command = command.clone();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				"recover_unknown_quick_task_turn",
				request.attempt_id.as_str(),
			)? {
				let recovered = serde_json::from_str(&response)
					.map_err(|_| incompatible("unknown Quick Task recovery receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(RecoverUnknownQuickTaskTurnOutcome::Replayed(recovered));
			}
			let authority: bool = transaction
				.query_row(
					"SELECT EXISTS (
					 SELECT 1 FROM conversations AS c
					 JOIN runtime_sessions AS s ON s.conversation_id = c.conversation_id
					 JOIN turns AS t ON t.runtime_session_id = s.runtime_session_id
					 JOIN provider_attempts AS p ON p.turn_id = t.turn_id
					 JOIN process_generations AS pg ON pg.generation_id = p.process_generation_id
					 JOIN process_generation_death_evidence AS d
					   ON d.generation_id = pg.generation_id AND d.evidence_id = pg.death_evidence_id
					 WHERE c.conversation_id = ?1 AND c.kind = 'ordinary_task' AND c.state = 'active'
					   AND c.revision = ?2 AND s.runtime_session_id = ?3 AND s.state = 'active'
					   AND s.revision = ?4 AND t.turn_id = ?5 AND t.role = 'user'
					   AND t.status = 'active' AND t.revision = ?6 AND p.attempt_id = ?7
					   AND p.state = 'unknown' AND p.revision = ?8
					   AND p.process_generation_id = ?9 AND pg.state = 'dead'
					   AND NOT EXISTS (
					     SELECT 1 FROM history_items AS h
					     WHERE h.turn_id = t.turn_id AND h.status = 'streaming'
					   )
					 )",
					params![
						request.conversation_id.as_str(),
						request.expected_conversation_revision,
						request.runtime_session_id.as_str(),
						request.expected_runtime_session_revision,
						request.user_turn_id.as_str(),
						request.expected_user_turn_revision,
						request.attempt_id.as_str(),
						request.expected_attempt_revision,
						request.process_generation_id.as_str(),
					],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if !authority || history_exists(&transaction, &request.history_item_id)? {
				return Ok(RecoverUnknownQuickTaskTurnOutcome::Rejected);
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = transaction
				.execute(
					"UPDATE turns SET status = 'failed', revision = revision + 1,
					 updated_at_micros = ?4, completed_at_micros = ?4
					 WHERE turn_id = ?1 AND revision = ?2 AND status = 'active'
					   AND conversation_id = ?3",
					params![
						request.user_turn_id.as_str(),
						request.expected_user_turn_revision,
						request.conversation_id.as_str(),
						now,
					],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Ok(RecoverUnknownQuickTaskTurnOutcome::Rejected);
			}
			let history_sequence: i64 = transaction
				.query_row(
					"SELECT COALESCE(MAX(sequence), 0) + 1 FROM history_items
					 WHERE conversation_id = ?1",
					params![request.conversation_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO history_items (
					 history_item_id, conversation_id, turn_id, sequence, kind, role, status,
					 media_type, inline_text, blob_sha256, metadata_json, revision,
					 created_at_micros, updated_at_micros
					 ) VALUES (?1, ?2, ?3, ?4, 'status', 'user', 'completed',
					 'text/plain', ?5, NULL, ?6, 1, ?7, ?7)",
					params![
						request.history_item_id.as_str(),
						request.conversation_id.as_str(),
						request.user_turn_id.as_str(),
						history_sequence,
						"Previous turn was interrupted. You can continue.",
						metadata_json(&HistoryMetadata::empty())?,
						now,
					],
				)
				.map_err(sql_error)?;
			touch_conversation(&transaction, &request.conversation_id, now)?;
			let recovered = RecoveredUnknownQuickTaskTurn {
				turn_revision: request.expected_user_turn_revision + 1,
			};
			write_receipt(
				&transaction,
				&command,
				"recover_unknown_quick_task_turn",
				request.attempt_id.as_str(),
				&serde_json::to_string(&recovered)
					.map_err(|_| incompatible("unknown Quick Task recovery receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(RecoverUnknownQuickTaskTurnOutcome::Applied(recovered))
		})
		.await
	}

	/// Close one provider-less starting session after all possible external effects are terminal.
	pub async fn archive_local_quick_task_conversation(
		&self,
		command: &CommandIdentity,
		request: &ArchiveLocalQuickTaskConversation,
	) -> Result<ArchiveLocalQuickTaskConversationOutcome, StoreError> {
		if request.expected_conversation_revision <= 0
			|| request.expected_runtime_session_revision <= 0
		{
			return Err(StoreError::InvalidInput(
				"local Quick Task archive coordinates are invalid",
			));
		}
		let command = command.clone();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) = read_receipt(
				&transaction,
				&command,
				"archive_local_quick_task_conversation",
				request.conversation_id.as_str(),
			)? {
				let archived = serde_json::from_str(&response)
					.map_err(|_| incompatible("local Quick Task archive receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(ArchiveLocalQuickTaskConversationOutcome::Replayed(archived));
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let session_changed = transaction
				.execute(
					"UPDATE runtime_sessions SET state = 'ended', revision = revision + 1,
					 updated_at_micros = ?5, ended_at_micros = ?5
					 WHERE runtime_session_id = ?1 AND conversation_id = ?2 AND revision = ?4
					   AND state = 'starting' AND codex_thread_id IS NULL
					   AND thread_start_fence_key IS NULL AND thread_start_request_id IS NULL
					   AND thread_start_response_id IS NULL
					   AND EXISTS (
					     SELECT 1 FROM conversations AS c WHERE c.conversation_id = ?2
					       AND c.state = 'active' AND c.revision = ?3
					   )
					   AND NOT EXISTS (
					     SELECT 1 FROM turns AS t WHERE t.conversation_id = ?2 AND t.status = 'active'
					   )
					   AND NOT EXISTS (
					     SELECT 1 FROM provider_attempts AS p WHERE p.conversation_id = ?2
					   )
					   AND NOT EXISTS (
					     SELECT 1 FROM process_generations AS p
					     WHERE p.runtime_session_id = ?1 AND p.state <> 'dead'
					   )",
					params![
						request.runtime_session_id.as_str(),
						request.conversation_id.as_str(),
						request.expected_conversation_revision,
						request.expected_runtime_session_revision,
						now,
					],
				)
				.map_err(sql_error)?;
			let conversation_changed = transaction
				.execute(
					"UPDATE conversations SET state = 'archived', revision = revision + 1,
					 updated_at_micros = ?3 WHERE conversation_id = ?1 AND revision = ?2
					 AND state = 'active'",
					params![
						request.conversation_id.as_str(),
						request.expected_conversation_revision,
						now,
					],
				)
				.map_err(sql_error)?;
			if session_changed != 1 || conversation_changed != 1 {
				return Ok(ArchiveLocalQuickTaskConversationOutcome::Rejected);
			}
			let archived = ArchivedQuickTaskConversation {
				conversation_id: request.conversation_id,
				conversation_revision: request.expected_conversation_revision + 1,
			};
			write_receipt(
				&transaction,
				&command,
				"archive_local_quick_task_conversation",
				archived.conversation_id.as_str(),
				&serde_json::to_string(&archived)
					.map_err(|_| incompatible("local Quick Task archive receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ArchiveLocalQuickTaskConversationOutcome::Applied(archived))
		})
		.await
	}

	pub async fn create_quick_task_routing_successor(
		&self,
		idempotency_key: &str,
		request: &CreateQuickTaskRoutingSuccessor,
	) -> Result<QuickTaskRoutingSuccessorOutcome, StoreError> {
		validate_key(idempotency_key)?;
		if request.expected_source_revision <= 0 {
			return Err(StoreError::InvalidInput(
				"routing successor source revision must be positive",
			));
		}
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = digest(&[
				request.source_conversation_id.as_str(),
				&request.expected_source_revision.to_string(),
			]);
			if let Some((stored_sha, successor_id)) = transaction.query_row(
				"SELECT request_sha256, successor_conversation_id
				 FROM conversation_routing_successors WHERE idempotency_key = ?1",
				params![key], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
			).optional().map_err(sql_error)? {
				if stored_sha != request_sha { return Err(StoreError::IdempotencyConflict); }
				let successor = read_routing_successor(&transaction, &request.source_conversation_id, &successor_id)?;
				transaction.commit().map_err(sql_error)?;
				return Ok(QuickTaskRoutingSuccessorOutcome::Replayed(successor));
			}
			let source = transaction.query_row(
				"SELECT c.title, q.message, q.working_directory, q.model, q.reasoning_effort,
				        q.fast, d.routing_decision_id,
				        d.decision_kind, c.revision
				 FROM conversations AS c
				 JOIN quick_task_requests AS q USING (conversation_id)
				 JOIN routing_decisions AS d ON d.conversation_id = c.conversation_id
				  AND d.authority_shape = 'conversation_account_registry'
				 WHERE c.conversation_id = ?1 AND c.state = 'active'",
				params![request.source_conversation_id.as_str()],
				|row| Ok((
					row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
					row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, bool>(5)?,
					row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, i64>(8)?,
				)),
			).optional().map_err(sql_error)?;
			let Some((title, message, working_directory, model, reasoning_effort, fast, routing_decision_id, decision_kind, revision)) = source else {
				return Ok(QuickTaskRoutingSuccessorOutcome::Rejected {
					code: "source_authority_unavailable".to_owned(), replayed: false,
				});
			};
			if revision != request.expected_source_revision || !matches!(decision_kind.as_str(), "waiting" | "no_route") {
				return Ok(QuickTaskRoutingSuccessorOutcome::Rejected {
					code: "source_authority_mismatch".to_owned(), replayed: false,
				});
			}
			let successor_id = random_uuid_v4()?;
			let initial_turn_id = random_uuid_v4()?;
			let now = unix_micros().map_err(StoreError::from)?;
			transaction.execute(
				"UPDATE conversations SET state = 'archived', revision = revision + 1,
				 updated_at_micros = ?2 WHERE conversation_id = ?1 AND revision = ?3",
				params![request.source_conversation_id.as_str(), now, revision],
			).map_err(sql_error)?;
			transaction.execute(
				"INSERT INTO conversations (conversation_id, kind, state, title, revision, created_at_micros, updated_at_micros)
				 VALUES (?1, 'ordinary_task', 'active', ?2, 1, ?3, ?3)",
				params![successor_id, title, now],
			).map_err(sql_error)?;
			transaction.execute(
				"INSERT INTO quick_task_requests (
				 conversation_id, operation_key, correlation_id, causation_id, initial_turn_id,
				 message, working_directory, model, reasoning_effort, fast, created_at_micros
				 ) VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
				params![successor_id, key, request.source_conversation_id.as_str(), initial_turn_id, message, working_directory, model, reasoning_effort, fast, now],
			).map_err(sql_error)?;
			transaction.execute(
				"INSERT INTO conversation_routing_successors (
				 source_conversation_id, successor_conversation_id, source_routing_decision_id,
				 idempotency_key, request_sha256, created_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				params![request.source_conversation_id.as_str(), successor_id, routing_decision_id, key, request_sha, now],
			).map_err(sql_error)?;
			let successor = QuickTaskRoutingSuccessor {
				source_conversation_id: request.source_conversation_id,
				source_revision: revision + 1,
				successor_conversation_id: ConversationId::new(successor_id)
					.map_err(|_| incompatible("routing successor identity"))?,
				successor_revision: 1,
				source_routing_decision_id: routing_decision_id,
			};
			transaction.commit().map_err(sql_error)?;
			Ok(QuickTaskRoutingSuccessorOutcome::Fresh(successor))
		}).await
	}

	pub async fn admit_initial_quick_task_turn(
		&self,
		blob_store: &BlobStore,
		idempotency_key: &str,
		request: &AdmitInitialQuickTaskTurn,
	) -> Result<InitialQuickTaskTurnAdmissionOutcome, StoreError> {
		validate_key(idempotency_key)?;
		validate_initial_admission(request)?;
		let payload = publish_payload(blob_store, &request.message.text)?;
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = initial_admission_digest(&request, &payload);
			if let Some(response) = read_runtime_receipt(
				&transaction,
				&key,
				&request_sha,
				"admit_initial_quick_task_turn",
				request.message.conversation_id.as_str(),
			)? {
				let admission: InitialQuickTaskTurnAdmissionReadback =
					serde_json::from_str(&response)
						.map_err(|_| incompatible("initial admission receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(InitialQuickTaskTurnAdmissionOutcome::Replayed(admission));
			}
			let authority = transaction
				.query_row(
					"SELECT p.routing_decision_id
				 FROM continuation_plans AS p
				 JOIN runtime_sessions AS s ON s.runtime_session_id = p.runtime_session_id
				 JOIN conversations AS c ON c.conversation_id = p.conversation_id
				 WHERE p.continuation_plan_id = ?1 AND p.kind = 'initial_thread'
				   AND p.conversation_id = ?2 AND p.turn_id = ?3
				   AND c.state = 'active' AND c.revision = ?4
				   AND s.revision = ?5 AND s.state = 'starting'",
					params![
						request.continuation_plan_id,
						request.message.conversation_id.as_str(),
						request.message.turn_id.as_str(),
						request.expected_conversation_revision,
						request.expected_runtime_session_revision,
					],
					|row| row.get::<_, String>(0),
				)
				.optional()
				.map_err(sql_error)?;
			let Some(routing_decision_id) = authority else {
				return Ok(InitialQuickTaskTurnAdmissionOutcome::Rejected {
					rejection: InitialQuickTaskTurnAdmissionRejection::AuthorityUnavailable,
					replayed: false,
				});
			};
			if read_turn(&transaction, &request.message.turn_id)?.is_some()
				|| history_exists(&transaction, &request.message.history_item_id)?
			{
				return Ok(InitialQuickTaskTurnAdmissionOutcome::Rejected {
					rejection: InitialQuickTaskTurnAdmissionRejection::InitialAdmissionConflict,
					replayed: false,
				});
			}
			let now = unix_micros().map_err(StoreError::from)?;
			insert_turn(&transaction, &request.message, now)?;
			insert_history(&transaction, &request.message, &payload, now)?;
			touch_conversation(&transaction, &request.message.conversation_id, now)?;
			let admission = InitialQuickTaskTurnAdmissionReadback {
				routing_decision_id,
				continuation_plan_id: request.continuation_plan_id,
				turn: TurnReservationReadback {
					turn_id: request.message.turn_id,
					sequence: 1,
					status: TurnStatus::Active,
					revision: 1,
				},
				history_item_id: request.message.history_item_id,
			};
			write_runtime_receipt(
				&transaction,
				&key,
				&request_sha,
				"admit_initial_quick_task_turn",
				admission.turn.turn_id.as_str(),
				&serde_json::to_string(&admission)
					.map_err(|_| incompatible("initial admission receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(InitialQuickTaskTurnAdmissionOutcome::Fresh(admission))
		})
		.await
	}
}

fn validate_quick_task_conversation(
	create: &CreateQuickTaskConversation,
) -> Result<(), StoreError> {
	if create.title.is_empty()
		|| create.title.len() > 512
		|| create.message.is_empty()
		|| create.message.len() > 16_384
		|| create.working_directory.is_empty()
		|| create.working_directory.len() > 4_096
		|| !create.working_directory.starts_with('/')
		|| create.working_directory.chars().any(char::is_control)
		|| create.model.is_empty()
		|| create.model.len() > 128
		|| create.model.chars().any(char::is_control)
		|| !matches!(
			create.reasoning_effort.as_str(),
			"low" | "medium" | "high" | "xhigh" | "max" | "ultra"
		) {
		return Err(StoreError::InvalidInput("initial Quick Task Conversation request is invalid"));
	}
	credential_negative(&create.title)?;
	credential_negative(&create.message)
}

fn validate_initial_admission(request: &AdmitInitialQuickTaskTurn) -> Result<(), StoreError> {
	let message = &request.message;
	if request.expected_conversation_revision <= 0
		|| request.expected_runtime_session_revision != 1
		|| message.turn_sequence != 1
		|| message.turn_role != TurnRole::User
		|| message.possible_side_effects != PossibleSideEffects::Unknown
		|| message.ordinal != 0
		|| message.kind != HistoryItemKind::Message
		|| message.status != ItemStatus::Completed
		|| message.expected_revision.is_some()
		|| message.artifact.is_some()
	{
		return Err(StoreError::InvalidInput("initial Quick Task admission shape is invalid"));
	}
	validate_history_item(message)
}

fn validate_terminalization(request: &TerminalizeQuickTaskTurn) -> Result<(), StoreError> {
	if request.expected_conversation_revision <= 0
		|| request.expected_runtime_session_revision <= 0
		|| request.expected_user_turn_revision <= 0
		|| request.expected_provider_attempt_revision <= 0
		|| request.assistant_turn.as_ref().is_some_and(|(_, revision)| *revision <= 0)
		|| request.provider_thread_id.is_empty()
		|| request.provider_thread_id.len() > 512
		|| request.provider_turn_id.is_empty()
		|| request.provider_turn_id.len() > 256
	{
		return Err(StoreError::InvalidInput("Quick Task terminalization coordinates are invalid"));
	}
	Ok(())
}

fn validate_history_item(mutation: &RecordHistoryItem) -> Result<(), StoreError> {
	if mutation.turn_sequence <= 0
		|| mutation.ordinal < 0
		|| mutation.text.len() > MAX_BLOB_BYTES
		|| mutation.expected_revision.is_some_and(|revision| revision <= 0)
		|| !matches!(mutation.turn_role, TurnRole::User | TurnRole::Assistant)
		|| mutation.artifact.is_some()
	{
		return Err(StoreError::InvalidInput(
			"history item is invalid for the local Quick Task slice",
		));
	}
	credential_negative(&mutation.text)?;
	serde_json::to_string(&mutation.metadata)
		.map_err(|_| StoreError::InvalidInput("history metadata is invalid"))?;
	Ok(())
}

fn publish_payload(blob_store: &BlobStore, text: &str) -> Result<Payload, StoreError> {
	if text.len() <= MAX_INLINE_HISTORY_BYTES {
		return Ok(Payload { inline_text: Some(text.to_owned()), blob_hash: None });
	}
	let hash = blob_store.put(text.as_bytes())?;
	Ok(Payload { inline_text: None, blob_hash: Some(hash.to_hex()) })
}

fn insert_turn(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	now: i64,
) -> Result<(), StoreError> {
	transaction
		.execute(
			"INSERT INTO turns (
		 turn_id, conversation_id, runtime_session_id, sequence, role, possible_side_effects,
		 status, revision, created_at_micros, updated_at_micros
		 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', 1, ?7, ?7)",
			params![
				mutation.turn_id.as_str(),
				mutation.conversation_id.as_str(),
				mutation.runtime_session_id.as_str(),
				mutation.turn_sequence,
				turn_role_text(mutation.turn_role),
				side_effect_text(mutation.possible_side_effects),
				now,
			],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn insert_history(
	transaction: &Transaction<'_>,
	mutation: &RecordHistoryItem,
	payload: &Payload,
	now: i64,
) -> Result<(), StoreError> {
	let sequence: i64 = transaction
		.query_row(
			"SELECT COALESCE(MAX(sequence), 0) + 1 FROM history_items WHERE conversation_id = ?1",
			params![mutation.conversation_id.as_str()],
			|row| row.get(0),
		)
		.map_err(sql_error)?;
	transaction
		.execute(
			"INSERT INTO history_items (
		 history_item_id, conversation_id, turn_id, sequence, kind, role, status,
		 media_type, inline_text, blob_sha256, metadata_json, revision,
		 created_at_micros, updated_at_micros
		 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12)",
			params![
				mutation.history_item_id.as_str(),
				mutation.conversation_id.as_str(),
				mutation.turn_id.as_str(),
				sequence,
				history_kind_text(mutation.kind),
				turn_role_text(mutation.turn_role),
				item_status_text(mutation.status),
				mutation.media_type.as_str(),
				payload.inline_text,
				payload.blob_hash,
				metadata_json(&mutation.metadata)?,
				now,
			],
		)
		.map_err(sql_error)?;
	Ok(())
}

struct StoredTurnShape {
	conversation_id: String,
	runtime_session_id: Option<String>,
	sequence: i64,
	role: String,
	possible_side_effects: String,
	status: String,
	revision: i64,
}

fn read_turn(
	transaction: &Transaction<'_>,
	turn_id: &TurnId,
) -> Result<Option<StoredTurnShape>, StoreError> {
	transaction.query_row(
		"SELECT conversation_id, runtime_session_id, sequence, role, possible_side_effects, status, revision
		 FROM turns WHERE turn_id = ?1",
		params![turn_id.as_str()],
		|row| Ok(StoredTurnShape {
			conversation_id: row.get(0)?, runtime_session_id: row.get(1)?, sequence: row.get(2)?,
			role: row.get(3)?, possible_side_effects: row.get(4)?, status: row.get(5)?, revision: row.get(6)?,
		}),
	).optional().map_err(sql_error)
}

fn validate_existing_turn(
	turn: &StoredTurnShape,
	mutation: &RecordHistoryItem,
) -> Result<(), StoreError> {
	if turn.conversation_id != mutation.conversation_id.as_str()
		|| turn.runtime_session_id.as_deref() != Some(mutation.runtime_session_id.as_str())
		|| turn.sequence != mutation.turn_sequence
		|| turn.role != turn_role_text(mutation.turn_role)
		|| turn.possible_side_effects != side_effect_text(mutation.possible_side_effects)
		|| turn.status != "active"
		|| turn.revision != 1
	{
		return Err(incompatible("history Turn authority"));
	}
	Ok(())
}

fn history_exists(
	transaction: &Transaction<'_>,
	history_item_id: &HistoryItemId,
) -> Result<bool, StoreError> {
	transaction
		.query_row(
			"SELECT EXISTS (SELECT 1 FROM history_items WHERE history_item_id = ?1)",
			params![history_item_id.as_str()],
			|row| row.get(0),
		)
		.map_err(sql_error)
}

fn read_history_entry(
	transaction: &Transaction<'_>,
	history_item_id: &str,
) -> Result<Option<HistoryEntry>, StoreError> {
	transaction
		.query_row(
			"SELECT h.history_item_id, h.turn_id, t.runtime_session_id, t.role,
		 t.possible_side_effects, h.kind, h.status, h.inline_text, h.blob_sha256,
		 h.media_type, h.metadata_json, h.revision, h.sequence
		 FROM history_items AS h JOIN turns AS t USING (turn_id)
		 WHERE h.history_item_id = ?1",
			params![history_item_id],
			read_history_row,
		)
		.optional()
		.map_err(sql_error)
}

fn read_history_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
	let blob_hash = row
		.get::<_, Option<String>>(8)?
		.map(|value| BlobHash::parse(&value).map_err(|_| rusqlite::Error::InvalidQuery))
		.transpose()?;
	let media_type = HistoryMediaType::new(row.get::<_, String>(9)?)
		.map_err(|_| rusqlite::Error::InvalidQuery)?;
	let metadata = serde_json::from_str::<HistoryMetadata>(&row.get::<_, String>(10)?)
		.map_err(|_| rusqlite::Error::InvalidQuery)?;
	Ok(HistoryEntry {
		history_item_id: row.get(0)?,
		turn_id: row.get(1)?,
		runtime_session_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
		turn_role: parse_turn_role(&row.get::<_, String>(3)?)
			.map_err(|_| rusqlite::Error::InvalidQuery)?,
		possible_side_effects: parse_side_effect(&row.get::<_, String>(4)?)
			.map_err(|_| rusqlite::Error::InvalidQuery)?,
		kind: parse_history_kind(&row.get::<_, String>(5)?)
			.map_err(|_| rusqlite::Error::InvalidQuery)?,
		status: parse_item_status(&row.get::<_, String>(6)?)
			.map_err(|_| rusqlite::Error::InvalidQuery)?,
		inline_text: row.get(7)?,
		blob_hash,
		blob_byte_length: None,
		media_type,
		metadata,
		artifact: None,
		revision: row.get(11)?,
	})
}

fn hydrate_history_blob(
	blob_store: &BlobStore,
	entry: &mut HistoryEntry,
) -> Result<(), StoreError> {
	if let Some(hash) = entry.blob_hash {
		let bytes = blob_store.read(hash)?;
		entry.blob_byte_length =
			Some(u64::try_from(bytes.len()).map_err(|_| incompatible("history blob length"))?);
	}
	Ok(())
}

fn verify_history_blob(blob_store: &BlobStore, entry: &HistoryEntry) -> Result<(), StoreError> {
	if let Some(hash) = entry.blob_hash {
		let bytes = blob_store.read(hash)?;
		if entry.inline_text.is_some() || bytes.len() > MAX_BLOB_BYTES {
			return Err(incompatible("history blob"));
		}
	} else if entry.inline_text.is_none() {
		return Err(incompatible("history payload"));
	}
	Ok(())
}

fn metadata_json(metadata: &HistoryMetadata) -> Result<String, StoreError> {
	serde_json::to_string(metadata)
		.map_err(|_| StoreError::InvalidInput("history metadata is invalid"))
}

fn touch_conversation(
	transaction: &Transaction<'_>,
	id: &ConversationId,
	now: i64,
) -> Result<(), StoreError> {
	let changed = transaction.execute(
		"UPDATE conversations SET updated_at_micros = ?2 WHERE conversation_id = ?1 AND state = 'active'",
		params![id.as_str(), now],
	).map_err(sql_error)?;
	if changed != 1 {
		return Err(incompatible("Conversation activity owner"));
	}
	Ok(())
}

fn read_receipt(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
	operation: &str,
	entity_id: &str,
) -> Result<Option<String>, StoreError> {
	read_runtime_receipt(transaction, &command.key, &command.request_hash, operation, entity_id)
}

fn read_runtime_receipt(
	transaction: &Transaction<'_>,
	key: &str,
	request_sha: &str,
	operation: &str,
	entity_id: &str,
) -> Result<Option<String>, StoreError> {
	let row = transaction
		.query_row(
			"SELECT request_sha256, operation, entity_id, response_json
		 FROM runtime_command_receipts WHERE idempotency_key = ?1",
			params![key],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let Some((stored_sha, stored_operation, stored_entity, response)) = row else {
		return Ok(None);
	};
	if stored_sha != request_sha || stored_operation != operation || stored_entity != entity_id {
		return Err(StoreError::IdempotencyConflict);
	}
	Ok(Some(response))
}

fn write_receipt(
	transaction: &Transaction<'_>,
	command: &CommandIdentity,
	operation: &str,
	entity_id: &str,
	response_json: &str,
	completed_at_micros: i64,
) -> Result<(), StoreError> {
	write_runtime_receipt(
		transaction,
		&command.key,
		&command.request_hash,
		operation,
		entity_id,
		response_json,
		completed_at_micros,
	)
}

fn write_runtime_receipt(
	transaction: &Transaction<'_>,
	key: &str,
	request_sha: &str,
	operation: &str,
	entity_id: &str,
	response_json: &str,
	completed_at_micros: i64,
) -> Result<(), StoreError> {
	transaction
		.execute(
			"INSERT INTO runtime_command_receipts (
		 idempotency_key, request_sha256, operation, entity_id, response_json, completed_at_micros
		 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![key, request_sha, operation, entity_id, response_json, completed_at_micros],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn read_routing_successor(
	transaction: &Transaction<'_>,
	source_id: &ConversationId,
	successor_id: &str,
) -> Result<QuickTaskRoutingSuccessor, StoreError> {
	let row = transaction
		.query_row(
			"SELECT s.revision, n.revision, r.source_routing_decision_id
		 FROM conversation_routing_successors AS r
		 JOIN conversations AS s ON s.conversation_id = r.source_conversation_id
		 JOIN conversations AS n ON n.conversation_id = r.successor_conversation_id
		 WHERE r.source_conversation_id = ?1 AND r.successor_conversation_id = ?2",
			params![source_id.as_str(), successor_id],
			|row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)),
		)
		.map_err(sql_error)?;
	Ok(QuickTaskRoutingSuccessor {
		source_conversation_id: source_id.clone(),
		source_revision: row.0,
		successor_conversation_id: ConversationId::new(successor_id.to_owned())
			.map_err(|_| incompatible("routing successor identity"))?,
		successor_revision: row.1,
		source_routing_decision_id: row.2,
	})
}

#[allow(clippy::too_many_lines)] // Keep the complete projection read and invariant checks together.
fn conversation_projection(
	connection: &rusqlite::Connection,
	row: (String, String, i64, i64),
) -> Result<OrdinaryTaskConversationProjection, StoreError> {
	let conversation_id = ConversationId::new(row.0)
		.map_err(|_| incompatible("ordinary Task Conversation identity"))?;
	if row.1 == "archived" {
		let successor = connection
			.query_row(
				"SELECT r.successor_conversation_id, c.revision
			 FROM conversation_routing_successors AS r
			 JOIN conversations AS c ON c.conversation_id = r.successor_conversation_id
			 WHERE r.source_conversation_id = ?1",
				params![conversation_id.as_str()],
				|row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
			)
			.optional()
			.map_err(sql_error)?;
		return match successor {
			Some(successor) => Ok(OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
				source_conversation_id: conversation_id,
				source_revision: row.2,
				successor_conversation_id: ConversationId::new(successor.0)
					.map_err(|_| incompatible("routing successor identity"))?,
				successor_conversation_revision: successor.1,
			}),
			None => Ok(OrdinaryTaskConversationProjection::Archived {
				conversation_id,
				conversation_revision: row.2,
			}),
		};
	}
	if row.1 != "active" || row.2 <= 0 || row.3 <= 0 {
		return Err(incompatible("ordinary Task Conversation lifecycle"));
	}
	let session = connection
		.query_row(
			"SELECT runtime_session_id, revision, state, has_acknowledged_turn
		 FROM runtime_sessions WHERE conversation_id = ?1 AND state IN ('starting', 'active')",
			params![conversation_id.as_str()],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, bool>(3)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let route = connection
		.query_row(
			"SELECT routing_decision_id, decision_kind, quota_classification
		 FROM routing_decisions WHERE conversation_id = ?1
		 ORDER BY created_at_micros DESC LIMIT 1",
			params![conversation_id.as_str()],
			|row| {
				Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let active_turn = connection
		.query_row(
			"SELECT turn_id, revision FROM turns
		 WHERE conversation_id = ?1 AND role = 'user' AND status = 'active'
		 ORDER BY sequence DESC LIMIT 1",
			params![conversation_id.as_str()],
			|row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
		)
		.optional()
		.map_err(sql_error)?;
	let has_admitted_user_turn: bool = connection
		.query_row(
			"SELECT EXISTS (SELECT 1 FROM turns WHERE conversation_id = ?1 AND role = 'user')",
			params![conversation_id.as_str()],
			|row| row.get(0),
		)
		.map_err(sql_error)?;
	let has_active_provider_attempt: bool = connection
		.query_row(
			"SELECT EXISTS (SELECT 1 FROM provider_attempts WHERE conversation_id = ?1
		 AND state IN ('prepared', 'dispatch_authorized'))",
			params![conversation_id.as_str()],
			|row| row.get(0),
		)
		.map_err(sql_error)?;
	let has_unknown_provider_attempt: bool = connection
		.query_row(
			"SELECT EXISTS (
		 SELECT 1 FROM provider_attempts AS p JOIN turns AS t ON t.turn_id = p.turn_id
		 WHERE p.conversation_id = ?1 AND p.state = 'unknown' AND t.status = 'active'
		 )",
			params![conversation_id.as_str()],
			|row| row.get(0),
		)
		.map_err(sql_error)?;
	let (
		runtime_session_id,
		runtime_session_revision,
		runtime_session_state,
		has_acknowledged_turn,
	) = match session {
		Some((id, revision, state, acknowledged)) => (
			Some(RuntimeSessionId::new(id).map_err(|_| incompatible("RuntimeSession identity"))?),
			Some(revision),
			Some(parse_runtime_session_state(&state)?),
			acknowledged,
		),
		None => (None, None, None, false),
	};
	let routing_decision_id = route.as_ref().map(|value| value.0.clone());
	let pre_session_state = if runtime_session_id.is_some() {
		None
	} else {
		Some(match route.as_ref() {
			None => OrdinaryTaskPreSessionState::RoutingPending,
			Some((_, decision, _)) if decision == "selected" =>
				OrdinaryTaskPreSessionState::EstablishmentPending,
			Some((_, _, quota)) if quota == "known_depleted" =>
				OrdinaryTaskPreSessionState::QuotaExhausted,
			Some(_) => OrdinaryTaskPreSessionState::NoRoute,
		})
	};
	Ok(OrdinaryTaskConversationProjection::Current(OrdinaryTaskConversationReadback {
		conversation_id,
		conversation_revision: row.2,
		runtime_session_id,
		runtime_session_revision,
		runtime_session_state,
		has_acknowledged_turn,
		active_turn_id: active_turn
			.as_ref()
			.map(|turn| TurnId::new(turn.0.clone()))
			.transpose()
			.map_err(|_| incompatible("active Turn identity"))?,
		active_turn_revision: active_turn.map(|turn| turn.1),
		has_admitted_user_turn,
		has_active_provider_attempt,
		has_unknown_provider_attempt,
		pre_session_state,
		routing_decision_id,
		updated_at_micros: row.3,
	}))
}

fn turn_matches(
	transaction: &Transaction<'_>,
	turn_id: &TurnId,
	conversation_id: &ConversationId,
	runtime_session_id: &RuntimeSessionId,
	revision: i64,
	role: TurnRole,
) -> Result<bool, StoreError> {
	transaction
		.query_row(
			"SELECT EXISTS (
		 SELECT 1 FROM turns WHERE turn_id = ?1 AND conversation_id = ?2
		 AND runtime_session_id = ?3 AND revision = ?4 AND role = ?5 AND status = 'active'
		 )",
			params![
				turn_id.as_str(),
				conversation_id.as_str(),
				runtime_session_id.as_str(),
				revision,
				turn_role_text(role)
			],
			|row| row.get(0),
		)
		.map_err(sql_error)
}

fn initial_admission_digest(request: &AdmitInitialQuickTaskTurn, payload: &Payload) -> String {
	digest(&[
		&request.expected_conversation_revision.to_string(),
		&request.expected_runtime_session_revision.to_string(),
		&request.continuation_plan_id,
		request.message.conversation_id.as_str(),
		request.message.runtime_session_id.as_str(),
		request.message.turn_id.as_str(),
		request.message.history_item_id.as_str(),
		payload.inline_text.as_deref().unwrap_or_default(),
		payload.blob_hash.as_deref().unwrap_or_default(),
	])
}

fn terminalization_digest(request: &TerminalizeQuickTaskTurn) -> String {
	let assistant_id =
		request.assistant_turn.as_ref().map(|value| value.0.as_str()).unwrap_or_default();
	let assistant_revision =
		request.assistant_turn.as_ref().map(|value| value.1.to_string()).unwrap_or_default();
	digest(&[
		request.conversation_id.as_str(),
		&request.expected_conversation_revision.to_string(),
		request.runtime_session_id.as_str(),
		&request.expected_runtime_session_revision.to_string(),
		request.user_turn_id.as_str(),
		&request.expected_user_turn_revision.to_string(),
		assistant_id,
		&assistant_revision,
		request.provider_attempt_id.as_str(),
		&request.expected_provider_attempt_revision.to_string(),
		request.provider_evidence_id.as_str(),
		provider_outcome_text(request.provider_outcome),
		&request.provider_thread_id,
		&request.provider_turn_id,
	])
}

fn digest(parts: &[&str]) -> String {
	let mut hasher = Sha256::new();
	for part in parts {
		hasher.update((part.len() as u64).to_be_bytes());
		hasher.update(part.as_bytes());
	}
	hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_key(value: &str) -> Result<(), StoreError> {
	if value.is_empty() || value.len() > 256 || decodex_core::contains_credential_material(value) {
		return Err(StoreError::InvalidInput("idempotency key is invalid"));
	}
	Ok(())
}

fn credential_negative(value: &str) -> Result<(), StoreError> {
	if decodex_core::contains_credential_material(value) {
		Err(StoreError::CredentialRejected)
	} else {
		Ok(())
	}
}

const fn turn_role_text(value: TurnRole) -> &'static str {
	match value {
		TurnRole::User => "user",
		TurnRole::Assistant => "assistant",
		TurnRole::System => "system",
		TurnRole::Tool => "tool",
	}
}

fn parse_turn_role(value: &str) -> Result<TurnRole, StoreError> {
	match value {
		"user" => Ok(TurnRole::User),
		"assistant" => Ok(TurnRole::Assistant),
		_ => Err(incompatible("history role")),
	}
}

const fn side_effect_text(value: PossibleSideEffects) -> &'static str {
	match value {
		PossibleSideEffects::None => "none",
		PossibleSideEffects::Possible => "possible",
		PossibleSideEffects::Unknown => "unknown",
	}
}

fn parse_side_effect(value: &str) -> Result<PossibleSideEffects, StoreError> {
	match value {
		"none" => Ok(PossibleSideEffects::None),
		"possible" => Ok(PossibleSideEffects::Possible),
		"unknown" => Ok(PossibleSideEffects::Unknown),
		_ => Err(incompatible("side-effect classification")),
	}
}

const fn history_kind_text(value: HistoryItemKind) -> &'static str {
	match value {
		HistoryItemKind::Message => "message",
		HistoryItemKind::Reasoning => "reasoning",
		HistoryItemKind::ToolCall => "tool_call",
		HistoryItemKind::ToolResult => "tool_result",
		HistoryItemKind::Artifact => "artifact",
		HistoryItemKind::Status => "status",
	}
}

fn parse_history_kind(value: &str) -> Result<HistoryItemKind, StoreError> {
	match value {
		"message" => Ok(HistoryItemKind::Message),
		"reasoning" => Ok(HistoryItemKind::Reasoning),
		"tool_call" => Ok(HistoryItemKind::ToolCall),
		"tool_result" => Ok(HistoryItemKind::ToolResult),
		"artifact" => Ok(HistoryItemKind::Artifact),
		"status" => Ok(HistoryItemKind::Status),
		_ => Err(incompatible("history kind")),
	}
}

const fn item_status_text(value: ItemStatus) -> &'static str {
	match value {
		ItemStatus::Streaming => "streaming",
		ItemStatus::Completed => "completed",
		ItemStatus::Failed => "failed",
	}
}

fn parse_item_status(value: &str) -> Result<ItemStatus, StoreError> {
	match value {
		"streaming" => Ok(ItemStatus::Streaming),
		"completed" => Ok(ItemStatus::Completed),
		"failed" => Ok(ItemStatus::Failed),
		_ => Err(incompatible("history status")),
	}
}

const fn turn_status_text(value: TurnStatus) -> &'static str {
	match value {
		TurnStatus::Active => "active",
		TurnStatus::Completed => "completed",
		TurnStatus::Failed => "failed",
	}
}

fn parse_turn_status(value: &str) -> Result<TurnStatus, StoreError> {
	match value {
		"active" => Ok(TurnStatus::Active),
		"completed" => Ok(TurnStatus::Completed),
		"failed" => Ok(TurnStatus::Failed),
		_ => Err(incompatible("Turn status")),
	}
}

fn parse_runtime_session_state(value: &str) -> Result<RuntimeSessionState, StoreError> {
	match value {
		"starting" => Ok(RuntimeSessionState::Starting),
		"active" => Ok(RuntimeSessionState::Active),
		"ended" => Ok(RuntimeSessionState::Ended),
		"diverged" => Ok(RuntimeSessionState::Diverged),
		_ => Err(incompatible("RuntimeSession state")),
	}
}

const fn provider_outcome_text(value: ProviderTerminalOutcome) -> &'static str {
	match value {
		ProviderTerminalOutcome::Succeeded => "succeeded",
		ProviderTerminalOutcome::FailedDefinitive => "failed_definitive",
		ProviderTerminalOutcome::NotSubmitted => "not_submitted",
	}
}

fn parse_provider_outcome(value: &str) -> Result<ProviderTerminalOutcome, StoreError> {
	match value {
		"succeeded" => Ok(ProviderTerminalOutcome::Succeeded),
		"failed_definitive" => Ok(ProviderTerminalOutcome::FailedDefinitive),
		"not_submitted" => Ok(ProviderTerminalOutcome::NotSubmitted),
		_ => Err(incompatible("provider outcome")),
	}
}

fn incompatible(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}

impl SqliteStore {
	#[allow(clippy::too_many_lines)] // Keep one atomic turn-and-history terminalization together.
	pub async fn terminalize_quick_task_turn(
		&self,
		idempotency_key: &str,
		request: &TerminalizeQuickTaskTurn,
	) -> Result<QuickTaskTerminalizationOutcome, StoreError> {
		validate_key(idempotency_key)?;
		validate_terminalization(request)?;
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = terminalization_digest(&request);
			if let Some(response) = read_runtime_receipt(
				&transaction,
				&key,
				&request_sha,
				"terminalize_quick_task_turn",
				request.provider_attempt_id.as_str(),
			)? {
				let readback: QuickTaskTerminalizationReadback = serde_json::from_str(&response)
					.map_err(|_| incompatible("terminalization receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(QuickTaskTerminalizationOutcome::Replayed(readback));
			}
			let expected_state = match request.provider_outcome {
				ProviderTerminalOutcome::Succeeded => "succeeded",
				ProviderTerminalOutcome::FailedDefinitive => "failed_definitive",
				ProviderTerminalOutcome::NotSubmitted => "not_submitted",
			};
			let attempt_matches: bool = transaction
				.query_row(
					"SELECT EXISTS (
				   SELECT 1 FROM provider_attempts AS p
				   JOIN provider_attempt_positive_evidence AS e ON e.attempt_id = p.attempt_id
				   WHERE p.attempt_id = ?1 AND p.conversation_id = ?2 AND p.turn_id = ?3
				     AND p.runtime_session_id = ?4 AND p.revision = ?5 AND p.state = ?6
				     AND e.evidence_id = ?7 AND e.provider_thread_id = ?8
				     AND e.provider_turn_id = ?9
				 )",
					params![
						request.provider_attempt_id.as_str(),
						request.conversation_id.as_str(),
						request.user_turn_id.as_str(),
						request.runtime_session_id.as_str(),
						request.expected_provider_attempt_revision,
						expected_state,
						request.provider_evidence_id.as_str(),
						request.provider_thread_id,
						request.provider_turn_id,
					],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			let session_matches: bool = transaction
				.query_row(
					"SELECT EXISTS (
				   SELECT 1 FROM runtime_sessions AS s JOIN conversations AS c USING (conversation_id)
				   WHERE s.runtime_session_id = ?1 AND s.conversation_id = ?2
				     AND s.revision = ?3 AND s.state = 'active' AND s.codex_thread_id = ?4
				     AND c.revision = ?5 AND c.state = 'active'
				 )",
					params![
						request.runtime_session_id.as_str(),
						request.conversation_id.as_str(),
						request.expected_runtime_session_revision,
						request.provider_thread_id,
						request.expected_conversation_revision,
					],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			let user_matches = turn_matches(
				&transaction,
				&request.user_turn_id,
				&request.conversation_id,
				&request.runtime_session_id,
				request.expected_user_turn_revision,
				TurnRole::User,
			)?;
			let assistant_matches =
				request.assistant_turn.as_ref().map_or(Ok(true), |(id, revision)| {
					turn_matches(
						&transaction,
						id,
						&request.conversation_id,
						&request.runtime_session_id,
						*revision,
						TurnRole::Assistant,
					)
				})?;
			if !attempt_matches || !session_matches || !user_matches || !assistant_matches {
				return Ok(QuickTaskTerminalizationOutcome::Rejected);
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let turn_state = match request.provider_outcome {
				ProviderTerminalOutcome::Succeeded => "completed",
				ProviderTerminalOutcome::FailedDefinitive
				| ProviderTerminalOutcome::NotSubmitted => "failed",
			};
			transaction
				.execute(
					"UPDATE turns SET status = ?2, revision = revision + 1,
				 updated_at_micros = ?3, completed_at_micros = ?3 WHERE turn_id = ?1",
					params![request.user_turn_id.as_str(), turn_state, now],
				)
				.map_err(sql_error)?;
			if let Some((assistant_turn_id, _)) = request.assistant_turn.as_ref() {
				transaction
					.execute(
						"UPDATE turns SET status = ?2, revision = revision + 1,
					 updated_at_micros = ?3, completed_at_micros = ?3 WHERE turn_id = ?1",
						params![assistant_turn_id.as_str(), turn_state, now],
					)
					.map_err(sql_error)?;
			}
			transaction.execute(
				"UPDATE runtime_sessions SET has_acknowledged_turn = 1, last_known_turn_id = ?2,
				 revision = revision + 1, updated_at_micros = ?3 WHERE runtime_session_id = ?1",
				params![request.runtime_session_id.as_str(), request.provider_turn_id, now],
			).map_err(sql_error)?;
			touch_conversation(&transaction, &request.conversation_id, now)?;
			let readback = QuickTaskTerminalizationReadback {
				runtime_session_revision: request.expected_runtime_session_revision + 1,
				user_turn_revision: request.expected_user_turn_revision + 1,
				assistant_turn_revision: request
					.assistant_turn
					.as_ref()
					.map(|(_, revision)| revision + 1),
				provider_attempt_revision: request.expected_provider_attempt_revision,
			};
			write_runtime_receipt(
				&transaction,
				&key,
				&request_sha,
				"terminalize_quick_task_turn",
				request.provider_attempt_id.as_str(),
				&serde_json::to_string(&readback)
					.map_err(|_| incompatible("terminalization receipt"))?,
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(QuickTaskTerminalizationOutcome::Applied(readback))
		})
		.await
	}

	/// SQLite terminalization is one transaction, so there is no partially committed work.
	pub async fn reconcile_quick_task_terminalizations(
		&self,
		limit: u16,
	) -> Result<u16, StoreError> {
		if !(1..=256).contains(&limit) {
			return Err(StoreError::InvalidInput("Quick Task terminalization bound is invalid"));
		}
		Ok(0)
	}

	pub async fn read_ordinary_task_conversations(
		&self,
		conversation_id: Option<&ConversationId>,
		after: Option<&OrdinaryTaskConversationCursor>,
		limit: usize,
	) -> Result<Vec<OrdinaryTaskConversationProjection>, StoreError> {
		if limit == 0 || limit > 65 || conversation_id.is_some() && after.is_some() {
			return Err(StoreError::InvalidInput(
				"ordinary Task Conversation read bound is invalid",
			));
		}
		if after.is_some_and(|cursor| cursor.updated_at_micros <= 0) {
			return Err(StoreError::InvalidInput("ordinary Task Conversation cursor is invalid"));
		}
		let conversation_id = conversation_id.cloned();
		let after = after.cloned();
		self.run(move |connection| {
			let mut rows = Vec::new();
			if let Some(id) = conversation_id.as_ref() {
				let row = connection
					.query_row(
						"SELECT conversation_id, state, revision, updated_at_micros
					 FROM conversations WHERE conversation_id = ?1 AND kind = 'ordinary_task'",
						params![id.as_str()],
						|row| {
							Ok((
								row.get::<_, String>(0)?,
								row.get::<_, String>(1)?,
								row.get::<_, i64>(2)?,
								row.get::<_, i64>(3)?,
							))
						},
					)
					.optional()
					.map_err(sql_error)?;
				if let Some(row) = row {
					rows.push(row);
				}
			} else {
				let (after_time, after_id) = after.as_ref().map_or((i64::MAX, ""), |cursor| {
					(cursor.updated_at_micros, cursor.conversation_id.as_str())
				});
				let mut statement = connection
					.prepare(
						"SELECT conversation_id, state, revision, updated_at_micros
					 FROM conversations
					 WHERE kind = 'ordinary_task' AND state = 'active'
					   AND (?1 = 9223372036854775807 OR updated_at_micros < ?1
					     OR (updated_at_micros = ?1 AND conversation_id < ?2))
					 ORDER BY updated_at_micros DESC, conversation_id DESC LIMIT ?3",
					)
					.map_err(sql_error)?;
				let selected = statement
					.query_map(
						params![after_time, after_id, i64::try_from(limit).unwrap_or(65)],
						|row| {
							Ok((
								row.get::<_, String>(0)?,
								row.get::<_, String>(1)?,
								row.get::<_, i64>(2)?,
								row.get::<_, i64>(3)?,
							))
						},
					)
					.map_err(sql_error)?;
				for row in selected {
					rows.push(row.map_err(sql_error)?);
				}
			}
			rows.into_iter().map(|row| conversation_projection(connection, row)).collect()
		})
		.await
	}

	pub async fn conversation_history(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		after: Option<&HistoryCursor>,
		page_size: u16,
	) -> Result<HistoryPage, StoreError> {
		if page_size == 0 || page_size > MAX_PAGE_SIZE {
			return Err(StoreError::InvalidInput("history page size must be within 1..=100"));
		}
		let conversation_id = conversation_id.clone();
		let after_sequence = after.map_or(0, |cursor| cursor.sequence);
		let entries = self
			.run(move |connection| {
				let exists: bool = connection
					.query_row(
						"SELECT EXISTS (SELECT 1 FROM conversations WHERE conversation_id = ?1)",
						params![conversation_id.as_str()],
						|row| row.get(0),
					)
					.map_err(sql_error)?;
				if !exists {
					return Err(StoreError::InvalidInput("Conversation does not exist"));
				}
				let mut statement = connection
					.prepare(
						"SELECT h.history_item_id, h.turn_id, t.runtime_session_id, t.role,
				 t.possible_side_effects, h.kind, h.status, h.inline_text, h.blob_sha256,
				 h.media_type, h.metadata_json, h.revision, h.sequence
				 FROM history_items AS h JOIN turns AS t USING (turn_id)
				 WHERE h.conversation_id = ?1 AND h.sequence > ?2
				 ORDER BY h.sequence LIMIT ?3",
					)
					.map_err(sql_error)?;
				let rows = statement
					.query_map(
						params![conversation_id.as_str(), after_sequence, i64::from(page_size) + 1],
						read_history_row,
					)
					.map_err(sql_error)?;
				let mut entries = Vec::new();
				for row in rows {
					entries.push(row.map_err(sql_error)?);
				}
				Ok(entries)
			})
			.await?;
		let has_more = entries.len() > usize::from(page_size);
		let mut entries = entries.into_iter().take(usize::from(page_size)).collect::<Vec<_>>();
		for entry in &mut entries {
			hydrate_history_blob(blob_store, entry)?;
		}
		let next_cursor = has_more.then(|| HistoryCursor {
			sequence: after_sequence + i64::try_from(entries.len()).unwrap_or(i64::MAX),
		});
		Ok(HistoryPage { entries, next_cursor })
	}

	pub async fn recent_conversation_history(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		limit: u16,
	) -> Result<Vec<HistoryEntry>, StoreError> {
		self.recent_conversation_history_filtered(blob_store, conversation_id, None, limit).await
	}

	/// Read recent history without the items owned by one exact Turn.
	///
	/// Context Pack fallback uses this after it has durably admitted the successor user Turn. The
	/// exclusion prevents that new intent from appearing both inside the pack and as the live
	/// input.
	pub async fn recent_conversation_history_excluding_turn(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		excluded_turn_id: &TurnId,
		limit: u16,
	) -> Result<Vec<HistoryEntry>, StoreError> {
		self.recent_conversation_history_filtered(
			blob_store,
			conversation_id,
			Some(excluded_turn_id),
			limit,
		)
		.await
	}

	async fn recent_conversation_history_filtered(
		&self,
		blob_store: &BlobStore,
		conversation_id: &ConversationId,
		excluded_turn_id: Option<&TurnId>,
		limit: u16,
	) -> Result<Vec<HistoryEntry>, StoreError> {
		if limit == 0 || usize::from(limit) > MAX_CONTEXT_RECENT_ITEMS {
			return Err(StoreError::InvalidInput("recent history bound is invalid"));
		}
		let conversation_id = conversation_id.clone();
		let excluded_turn_id = excluded_turn_id.map(|turn_id| turn_id.as_str().to_owned());
		let mut entries = self
			.run(move |connection| {
				let mut statement = connection
					.prepare(
						"SELECT history_item_id, turn_id, runtime_session_id, role,
				 possible_side_effects, kind, status, inline_text, blob_sha256,
				 media_type, metadata_json, revision, sequence FROM (
				   SELECT h.history_item_id, h.turn_id, t.runtime_session_id, t.role,
				    t.possible_side_effects, h.kind, h.status, h.inline_text, h.blob_sha256,
				    h.media_type, h.metadata_json, h.revision, h.sequence
				   FROM history_items AS h JOIN turns AS t USING (turn_id)
				   WHERE h.conversation_id = ?1 AND (?2 IS NULL OR h.turn_id <> ?2)
				   ORDER BY h.sequence DESC LIMIT ?3
				 ) ORDER BY sequence",
					)
					.map_err(sql_error)?;
				let rows = statement
					.query_map(
						params![conversation_id.as_str(), excluded_turn_id, i64::from(limit),],
						read_history_row,
					)
					.map_err(sql_error)?;
				let mut entries = Vec::new();
				for row in rows {
					entries.push(row.map_err(sql_error)?);
				}
				Ok(entries)
			})
			.await?;
		for entry in &mut entries {
			hydrate_history_blob(blob_store, entry)?;
		}
		Ok(entries)
	}
}

impl SqliteStore {
	pub async fn record_history_item(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<HistoryEntry, StoreError> {
		self.record_history_item_command(blob_store, command, mutation)
			.await
			.map(|(entry, _)| entry)
	}

	pub async fn reserve_user_turn_with_history_item(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<TurnReservationOutcome, StoreError> {
		if mutation.turn_role != TurnRole::User
			|| mutation.possible_side_effects != PossibleSideEffects::Unknown
			|| mutation.expected_revision.is_some()
			|| mutation.status != ItemStatus::Completed
		{
			return Err(StoreError::InvalidInput("user Turn reservation history item is invalid"));
		}
		let existing = self.read_turn_reservation(mutation).await?;
		let mut exact = mutation.clone();
		if let Some(readback) = existing.as_ref() {
			exact.turn_sequence = readback.sequence;
		}
		let (_, fresh) = self.record_history_item_command(blob_store, command, &exact).await?;
		let readback = self
			.read_turn_reservation(&exact)
			.await?
			.ok_or_else(|| incompatible("reserved user Turn readback"))?;
		if fresh {
			if existing.is_some()
				|| readback.sequence != mutation.turn_sequence
				|| readback.status != TurnStatus::Active
				|| readback.revision != 1
			{
				return Err(incompatible("fresh user Turn reservation"));
			}
			Ok(TurnReservationOutcome::Fresh(readback))
		} else {
			Ok(TurnReservationOutcome::Replayed(readback))
		}
	}

	async fn read_turn_reservation(
		&self,
		mutation: &RecordHistoryItem,
	) -> Result<Option<TurnReservationReadback>, StoreError> {
		let conversation_id = mutation.conversation_id.clone();
		let runtime_session_id = mutation.runtime_session_id.clone();
		let turn_id = mutation.turn_id.clone();
		self.run(move |connection| {
			let row = connection
				.query_row(
					"SELECT sequence, status, revision, role, possible_side_effects
				 FROM turns WHERE turn_id = ?1 AND conversation_id = ?2 AND runtime_session_id = ?3",
					params![
						turn_id.as_str(),
						conversation_id.as_str(),
						runtime_session_id.as_str()
					],
					|row| {
						Ok((
							row.get::<_, i64>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, i64>(2)?,
							row.get::<_, String>(3)?,
							row.get::<_, String>(4)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some((sequence, status, revision, role, side_effects)) = row else {
				return Ok(None);
			};
			if role != "user" || side_effects != "unknown" || sequence <= 0 || revision <= 0 {
				return Err(incompatible("Turn reservation"));
			}
			Ok(Some(TurnReservationReadback {
				turn_id,
				sequence,
				status: parse_turn_status(&status)?,
				revision,
			}))
		})
		.await
	}

	async fn record_history_item_command(
		&self,
		blob_store: &BlobStore,
		command: &CommandIdentity,
		mutation: &RecordHistoryItem,
	) -> Result<(HistoryEntry, bool), StoreError> {
		validate_history_item(mutation)?;
		let payload = publish_payload(blob_store, &mutation.text)?;
		let command = command.clone();
		let mutation = mutation.clone();
		let (entry, fresh) = self
			.run(move |connection| {
				let transaction = connection
					.transaction_with_behavior(TransactionBehavior::Immediate)
					.map_err(sql_error)?;
				if let Some(response) = read_receipt(
					&transaction,
					&command,
					"record_history_item",
					mutation.history_item_id.as_str(),
				)? {
					let receipt: HistoryReceipt = serde_json::from_str(&response)
						.map_err(|_| incompatible("history receipt"))?;
					if receipt.history_item_id != mutation.history_item_id.as_str() {
						return Err(incompatible("history receipt identity"));
					}
					let entry =
						read_history_entry(&transaction, mutation.history_item_id.as_str())?
							.ok_or_else(|| incompatible("history receipt row"))?;
					transaction.commit().map_err(sql_error)?;
					return Ok((entry, false));
				}
				let now = unix_micros().map_err(StoreError::from)?;
				match mutation.expected_revision {
					None => {
						if history_exists(&transaction, &mutation.history_item_id)? {
							return Err(StoreError::RevisionConflict {
								entity: format!("history_item/{}", mutation.history_item_id),
								expected: None,
								actual: Some(1),
							});
						}
						if let Some(turn) = read_turn(&transaction, &mutation.turn_id)? {
							validate_existing_turn(&turn, &mutation)?;
						} else {
							insert_turn(&transaction, &mutation, now)?;
						}
						insert_history(&transaction, &mutation, &payload, now)?;
					},
					Some(expected) if expected > 0 => {
						let changed = transaction
							.execute(
								"UPDATE history_items SET status = ?2, media_type = ?3,
						 inline_text = ?4, blob_sha256 = ?5, metadata_json = ?6,
						 revision = revision + 1, updated_at_micros = ?7
						 WHERE history_item_id = ?1 AND conversation_id = ?8 AND turn_id = ?9
						   AND revision = ?10",
								params![
									mutation.history_item_id.as_str(),
									item_status_text(mutation.status),
									mutation.media_type.as_str(),
									payload.inline_text,
									payload.blob_hash,
									metadata_json(&mutation.metadata)?,
									now,
									mutation.conversation_id.as_str(),
									mutation.turn_id.as_str(),
									expected,
								],
							)
							.map_err(sql_error)?;
						if changed != 1 {
							return Err(StoreError::RevisionConflict {
								entity: format!("history_item/{}", mutation.history_item_id),
								expected: Some(expected),
								actual: None,
							});
						}
					},
					Some(_) => {
						return Err(StoreError::InvalidInput("history revision must be positive"));
					},
				}
				touch_conversation(&transaction, &mutation.conversation_id, now)?;
				let entry = read_history_entry(&transaction, mutation.history_item_id.as_str())?
					.ok_or_else(|| incompatible("recorded history row"))?;
				write_receipt(
					&transaction,
					&command,
					"record_history_item",
					mutation.history_item_id.as_str(),
					&serde_json::to_string(&HistoryReceipt {
						history_item_id: mutation.history_item_id.as_str().to_owned(),
					})
					.map_err(|_| incompatible("history receipt"))?,
					now,
				)?;
				transaction.commit().map_err(sql_error)?;
				Ok((entry, true))
			})
			.await?;
		verify_history_blob(blob_store, &entry)?;
		Ok((entry, fresh))
	}

	pub async fn transition_turn(
		&self,
		command: &CommandIdentity,
		turn_id: &TurnId,
		expected_revision: i64,
		status: TurnStatus,
	) -> Result<i64, StoreError> {
		if expected_revision <= 0 || status == TurnStatus::Active {
			return Err(StoreError::InvalidInput("Turn transition is invalid"));
		}
		let command = command.clone();
		let turn_id = turn_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(response) =
				read_receipt(&transaction, &command, "transition_turn", turn_id.as_str())?
			{
				let revision = response.parse::<i64>().map_err(|_| incompatible("Turn receipt"))?;
				transaction.commit().map_err(sql_error)?;
				return Ok(revision);
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = transaction
				.execute(
					"UPDATE turns SET status = ?2, revision = revision + 1, updated_at_micros = ?3,
				 completed_at_micros = ?3
				 WHERE turn_id = ?1 AND revision = ?4 AND status = 'active'
				 AND NOT EXISTS (
				   SELECT 1 FROM history_items WHERE turn_id = ?1 AND status = 'streaming'
				 )",
					params![turn_id.as_str(), turn_status_text(status), now, expected_revision],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Err(StoreError::RevisionConflict {
					entity: format!("turn/{turn_id}"),
					expected: Some(expected_revision),
					actual: None,
				});
			}
			let revision = expected_revision + 1;
			write_receipt(
				&transaction,
				&command,
				"transition_turn",
				turn_id.as_str(),
				&revision.to_string(),
				now,
			)?;
			transaction.commit().map_err(sql_error)?;
			Ok(revision)
		})
		.await
	}
}

#[cfg(test)]
mod archive_tests {
	use decodex_core::{
		BlobStore, ContextPackInput, ContextPackPolicy, ContinuationCommandOutcome,
		ContinuationPlanKind, ConversationId, DecodexRoot, HistoryItemId, PinnedContextSource,
		PossibleSideEffects, ProcessExecutionEpochId, ProcessGenerationId, ProviderAttemptConsumer,
		ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState, ProviderDuplicateRisk,
		ProviderEvidenceId, ProviderEvidenceSource, ProviderPositiveEvidence, ProviderRequestId,
		ProviderRequestKey, ProviderRequestKeys, ProviderTerminalOutcome, RuntimeSessionId, TurnId,
		compile_context_pack,
	};
	use rusqlite::params;
	use tempfile::tempdir;

	use super::{
		ArchiveLocalQuickTaskConversation, ArchiveLocalQuickTaskConversationOutcome,
		ArchiveQuickTaskConversation, ArchiveQuickTaskConversationOutcome,
		CreateQuickTaskConversation, OrdinaryTaskConversationProjection,
		ReconcileStrandedQuickTaskTurn, ReconcileStrandedQuickTaskTurnOutcome,
		RecoverUnknownQuickTaskTurn, RecoverUnknownQuickTaskTurnOutcome, TerminalizeQuickTaskTurn,
	};
	use crate::{
		CommandIdentity, PlanContinuation, PrepareProviderAttemptOutcome,
		ProviderAttemptMutationOutcome, QuickTaskTerminalizationOutcome, SqliteStore,
		error::sqlite_error,
	};

	const CONVERSATION_ID: &str = "30000000-0000-4000-8000-000000000001";
	const RUNTIME_SESSION_ID: &str = "40000000-0000-4000-8000-000000000001";
	const TURN_ID: &str = "50000000-0000-4000-8000-000000000001";
	const ACCOUNT_ID: &str = "10000000-0000-4000-8000-000000000001";
	const ATTEMPT_ID: &str = "60000000-0000-4000-8000-000000000001";
	const GENERATION_ID: &str = "70000000-0000-4000-8000-000000000001";
	const INTERRUPTION_HISTORY_ID: &str = "80000000-0000-4000-8000-000000000001";
	const SUCCESSOR_TURN_ID: &str = "50000000-0000-4000-8000-000000000002";
	const SUCCESSOR_HISTORY_ID: &str = "80000000-0000-4000-8000-000000000002";

	async fn seed_provider_less_starting_task(store: &SqliteStore) {
		let conversation_id = ConversationId::new(CONVERSATION_ID).expect("conversation ID");
		store
			.create_quick_task_conversation(
				&CommandIdentity::new("create-local-fixture", b"create local fixture")
					.expect("create command"),
				&CreateQuickTaskConversation {
					conversation_id,
					work_item_id: None,
					title: "Local fixture".to_owned(),
					message: "Start this task.".to_owned(),
					working_directory: "/tmp".to_owned(),
					model: "gpt-5.6-sol".to_owned(),
					reasoning_effort: "high".to_owned(),
					fast: true,
				},
			)
			.await
			.expect("create conversation");
		store
			.with_connection(|connection| {
				connection
					.execute(
						"INSERT INTO account_identities (account_id, created_at_micros)
						 VALUES (?1, 1)",
						params![ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO accounts (
						 account_id, display_label, enabled, state, revision, provider,
						 provider_account_id, created_at_micros, updated_at_micros
						 ) VALUES (?1, 'Local fixture', 1, 'available', 1, 'chatgpt',
						 'local-fixture-provider', 1, 1)",
						params![ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO runtime_sessions (
						 runtime_session_id, conversation_id, account_id, account_revision,
						 account_snapshot_id, account_display_label, account_observed_state,
						 credential_binding_json, profile_snapshot_id, profile_revision,
						 profile_role, model, reasoning_effort, instructions, service_tier,
						 instructions_sha256, state, has_acknowledged_turn, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (
						 ?1, ?2, ?3, 1, '41000000-0000-4000-8000-000000000001',
						 'Local fixture', 'available', '{}',
						 '42000000-0000-4000-8000-000000000001', 1, 'task', 'gpt-5.6-sol',
						 'high', 'Follow the request.', 'priority',
						 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
						 'starting', 0, 3, 1, 1
						 )",
						params![RUNTIME_SESSION_ID, CONVERSATION_ID, ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("seed provider-less starting RuntimeSession");
	}

	fn seed_active_user_turn(store: &SqliteStore) {
		store
			.with_connection(|connection| {
				connection
					.execute(
						"INSERT INTO turns (
						 turn_id, conversation_id, runtime_session_id, sequence, role,
						 possible_side_effects, status, revision, created_at_micros,
						 updated_at_micros
						 ) VALUES (?1, ?2, ?3, 1, 'user', 'none', 'active', 1, 1, 1)",
						params![TURN_ID, CONVERSATION_ID, RUNTIME_SESSION_ID],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("seed active user Turn");
	}

	fn seed_unknown_provider_attempt(store: &SqliteStore) {
		store
			.with_connection(|connection| {
				connection
					.execute(
						"UPDATE runtime_sessions SET codex_thread_id = 'codex-thread-unknown',
						 state = 'active', thread_start_request_id = 1,
						 thread_start_request_sha256 = ?2, thread_start_response_id = 1,
						 thread_start_response_sha256 = ?2, has_acknowledged_turn = 1,
						 revision = 7 WHERE runtime_session_id = ?1",
						params![RUNTIME_SESSION_ID, "b".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO account_operations (
						 operation_id, account_id, kind, phase, provider, provider_account_id,
						 requested_display_label, requested_enabled, created_at_micros,
						 updated_at_micros, completed_at_micros
						 ) VALUES ('11000000-0000-4000-8000-000000000001', ?1, 'import',
						 'committed', 'chatgpt', 'local-fixture-provider', 'Local fixture', 1,
						 1, 1, 1)",
						params![ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO routing_decisions (
						 routing_decision_id, operation_id, idempotency_key, request_sha256,
						 authority_shape, conversation_id, turn_id, conversation_revision,
						 snapshot_id, snapshot_json, decision_kind, account_id, account_revision,
						 routing_revision, quota_classification, causes_json, exclusions_json,
						 created_at_micros
						 ) VALUES (
						 '21000000-0000-4000-8000-000000000001',
						 '22000000-0000-4000-8000-000000000001', 'unknown-route', ?4,
						 'conversation_account_registry', ?1, ?2, 1,
						 '23000000-0000-4000-8000-000000000001', '{}', 'selected', ?3, 1,
						 1, 'known_available', '[]', '[]', 1)",
						params![CONVERSATION_ID, TURN_ID, ACCOUNT_ID, "c".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO continuation_plans (
						 continuation_plan_id, operation_id, idempotency_key, request_sha256,
						 conversation_id, turn_id, routing_decision_id,
						 source_runtime_session_id, source_runtime_session_revision,
						 selected_account_id, runtime_session_id, kind, created_at_micros
						 ) VALUES (
						 '31000000-0000-4000-8000-000000000001',
						 '32000000-0000-4000-8000-000000000001', 'unknown-plan', ?4,
						 ?1, ?2, '21000000-0000-4000-8000-000000000001', ?3, 7, ?5, ?3,
						 'initial_thread', 1)",
						params![
							CONVERSATION_ID,
							TURN_ID,
							RUNTIME_SESSION_ID,
							"d".repeat(64),
							ACCOUNT_ID,
						],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO process_execution_epochs (
						 execution_epoch_id, authorization_sha256, created_at_micros
						 ) VALUES ('41000000-0000-4000-8000-000000000009', ?1, 1)",
						params!["e".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO process_generations (
						 generation_id, account_id, runtime_session_id, execution_epoch_id,
						 runner_identity, intended_boot_id, control_kind, isolation_kind,
						 account_revision, credential_schema_version, credential_version,
						 credential_fingerprint, credential_writer_operation_id, provider,
						 provider_account_id, refresh_callback_profile_sha256, state, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (?1, ?2, ?3, '41000000-0000-4000-8000-000000000009',
						 'test-runner', 'test-boot', 'stdio_only_best_effort_eof', 'session',
						 1, 1, 1, ?4, '11000000-0000-4000-8000-000000000001', 'chatgpt',
						 'local-fixture-provider', ?4, 'starting', 1, 1, 1)",
						params![GENERATION_ID, ACCOUNT_ID, RUNTIME_SESSION_ID, "f".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO provider_attempts (
						 attempt_id, conversation_id, turn_id, continuation_plan_id,
						 routing_decision_id, runtime_session_id, runtime_session_revision,
						 account_id, process_generation_id, process_generation_revision,
						 execution_epoch_id, request_id, request_sha256,
						 provider_correlation_key, state, unknown_reason, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (?1, ?2, ?3, '31000000-0000-4000-8000-000000000001',
						 '21000000-0000-4000-8000-000000000001', ?4, 7, ?5, ?6, 1,
						 '41000000-0000-4000-8000-000000000009',
						 '61000000-0000-4000-8000-000000000001', ?7,
						 'app-server:test:1', 'unknown', 'dispatch_outcome_unavailable', 1, 1, 1)",
						params![
							ATTEMPT_ID,
							CONVERSATION_ID,
							TURN_ID,
							RUNTIME_SESSION_ID,
							ACCOUNT_ID,
							GENERATION_ID,
							"1".repeat(64),
						],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("seed unknown provider attempt");
	}

	fn record_exact_process_death(store: &SqliteStore) {
		store
			.with_connection(|connection| {
				connection
					.execute(
						"INSERT INTO process_generation_death_evidence (
						 evidence_id, generation_id, kind, observed_boot_id, witness_sha256,
						 observed_at_micros
						 ) VALUES ('71000000-0000-4000-8000-000000000001', ?1,
						 'spawn_not_created', 'test-boot', ?2, 2)",
						params![GENERATION_ID, "2".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"UPDATE process_generations SET state = 'dead',
						 death_evidence_id = '71000000-0000-4000-8000-000000000001',
						 revision = 2, updated_at_micros = 2 WHERE generation_id = ?1",
						params![GENERATION_ID],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("record exact process death");
	}

	#[tokio::test]
	async fn verified_archive_atomically_closes_the_projection_and_replays_exactly() {
		let directory = tempdir().expect("temporary database directory");
		let store = SqliteStore::open_test(&directory.path().join("decodex.sqlite3"))
			.expect("initialize database");
		let conversation_id = ConversationId::new(CONVERSATION_ID).expect("conversation ID");
		let runtime_session_id =
			RuntimeSessionId::new(RUNTIME_SESSION_ID).expect("RuntimeSession ID");
		store
			.create_quick_task_conversation(
				&CommandIdentity::new("create-archive-fixture", b"create archive fixture")
					.expect("create command"),
				&CreateQuickTaskConversation {
					conversation_id: conversation_id.clone(),
					work_item_id: None,
					title: "Archive fixture".to_owned(),
					message: "Archive this task.".to_owned(),
					working_directory: "/tmp".to_owned(),
					model: "gpt-5.6-sol".to_owned(),
					reasoning_effort: "high".to_owned(),
					fast: true,
				},
			)
			.await
			.expect("create conversation");
		store
			.with_connection(|connection| {
				connection
					.execute(
						"INSERT INTO account_identities (account_id, created_at_micros)
						 VALUES (?1, 1)",
						params![ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO accounts (
						 account_id, display_label, enabled, state, revision, provider,
						 provider_account_id, created_at_micros, updated_at_micros
						 ) VALUES (?1, 'Archive fixture', 1, 'available', 1, 'chatgpt',
						 'archive-fixture-provider', 1, 1)",
						params![ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO runtime_sessions (
						 runtime_session_id, conversation_id, account_id, account_revision,
						 account_snapshot_id, account_display_label, account_observed_state,
						 credential_binding_json, profile_snapshot_id, profile_revision,
						 profile_role, model, reasoning_effort, instructions, service_tier,
						 instructions_sha256, codex_thread_id, state, thread_start_request_id,
						 thread_start_request_sha256, thread_start_response_id,
						 thread_start_response_sha256, has_acknowledged_turn, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (
						 ?1, ?2, ?3, 1, '41000000-0000-4000-8000-000000000001',
						 'Archive fixture', 'available', '{}',
						 '42000000-0000-4000-8000-000000000001', 1, 'task', 'gpt-5.6-sol',
						 'high', 'Follow the request.', 'priority',
						 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
						 'codex-thread-1', 'active', 1,
						 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1,
						 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
						 1, 7, 1, 1
						 )",
						params![RUNTIME_SESSION_ID, CONVERSATION_ID, ACCOUNT_ID],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("seed active RuntimeSession");

		let archive = ArchiveQuickTaskConversation {
			conversation_id: conversation_id.clone(),
			expected_conversation_revision: 1,
			runtime_session_id: runtime_session_id.clone(),
			expected_runtime_session_revision: 7,
		};
		let command =
			CommandIdentity::new("archive-fixture", b"archive fixture").expect("archive command");
		let archived = match store
			.archive_quick_task_conversation(&command, &archive)
			.await
			.expect("archive verified conversation")
		{
			ArchiveQuickTaskConversationOutcome::Applied(archived) => archived,
			other => panic!("archive was not applied: {other:?}"),
		};
		assert_eq!(archived.conversation_revision, 2);
		assert!(matches!(
			store
				.archive_quick_task_conversation(&command, &archive)
				.await
				.expect("replay archive command"),
			ArchiveQuickTaskConversationOutcome::Replayed(ref replayed)
				if replayed == &archived
		));

		let exact = store
			.read_ordinary_task_conversations(Some(&conversation_id), None, 1)
			.await
			.expect("read exact archived projection");
		assert!(matches!(
			exact.as_slice(),
			[OrdinaryTaskConversationProjection::Archived {
				conversation_id: exact_id,
				conversation_revision: 2,
			}] if exact_id == &conversation_id
		));
		assert!(
			store
				.read_ordinary_task_conversations(None, None, 65)
				.await
				.expect("list active projections")
				.is_empty()
		);
		assert_eq!(
			store.read_quick_task_request(&conversation_id).await.expect("read archived request"),
			None
		);
		let session: (String, i64, Option<i64>) = store
			.with_connection(|connection| {
				connection
					.query_row(
						"SELECT state, revision, ended_at_micros FROM runtime_sessions
						 WHERE runtime_session_id = ?1",
						params![RUNTIME_SESSION_ID],
						|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
					)
					.map_err(sqlite_error)
			})
			.expect("read ended RuntimeSession");
		assert_eq!(session.0, "ended");
		assert_eq!(session.1, 8);
		assert!(session.2.is_some());
	}

	#[tokio::test]
	async fn stranded_turn_reconciliation_requires_exact_inactive_owner_coordinates() {
		let directory = tempdir().expect("temporary database directory");
		let store = SqliteStore::open_test(&directory.path().join("decodex.sqlite3"))
			.expect("initialize database");
		seed_provider_less_starting_task(&store).await;
		seed_active_user_turn(&store);
		let request = ReconcileStrandedQuickTaskTurn {
			conversation_id: ConversationId::new(CONVERSATION_ID).expect("conversation ID"),
			expected_conversation_revision: 1,
			runtime_session_id: RuntimeSessionId::new(RUNTIME_SESSION_ID)
				.expect("RuntimeSession ID"),
			expected_runtime_session_revision: 3,
			turn_id: TurnId::new(TURN_ID).expect("Turn ID"),
			expected_turn_revision: 1,
		};
		let stale = ReconcileStrandedQuickTaskTurn {
			expected_runtime_session_revision: 2,
			..request.clone()
		};
		assert_eq!(
			store
				.reconcile_stranded_quick_task_turn(
					&CommandIdentity::new("stale-turn-reconcile", b"stale coordinates")
						.expect("stale command"),
					&stale,
				)
				.await
				.expect("reject stale coordinates"),
			ReconcileStrandedQuickTaskTurnOutcome::Rejected
		);

		let command = CommandIdentity::new("exact-turn-reconcile", b"exact coordinates")
			.expect("exact command");
		assert_eq!(
			store
				.reconcile_stranded_quick_task_turn(&command, &request)
				.await
				.expect("reconcile stranded Turn"),
			ReconcileStrandedQuickTaskTurnOutcome::Applied { turn_revision: 2 }
		);
		assert_eq!(
			store
				.reconcile_stranded_quick_task_turn(&command, &request)
				.await
				.expect("replay reconciliation"),
			ReconcileStrandedQuickTaskTurnOutcome::Replayed { turn_revision: 2 }
		);
		let turn: (String, i64, Option<i64>) = store
			.with_connection(|connection| {
				connection
					.query_row(
						"SELECT status, revision, completed_at_micros FROM turns WHERE turn_id = ?1",
						params![TURN_ID],
						|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
					)
					.map_err(sqlite_error)
			})
			.expect("read reconciled Turn");
		assert_eq!(turn.0, "failed");
		assert_eq!(turn.1, 2);
		assert!(turn.2.is_some());
	}

	#[tokio::test]
	async fn unknown_turn_recovery_requires_death_and_keeps_attempt_evidence() {
		let directory = tempdir().expect("temporary database directory");
		let store = SqliteStore::open_test(&directory.path().join("decodex.sqlite3"))
			.expect("initialize database");
		seed_provider_less_starting_task(&store).await;
		seed_active_user_turn(&store);
		seed_unknown_provider_attempt(&store);
		let conversation_id = ConversationId::new(CONVERSATION_ID).expect("conversation ID");
		let before = store
			.read_unknown_quick_task_attempt_for_recovery(&conversation_id)
			.await
			.expect("read active unknown attempt")
			.expect("unknown attempt exists");
		assert!(!before.process_generation_is_dead);
		let request = RecoverUnknownQuickTaskTurn {
			conversation_id: conversation_id.clone(),
			expected_conversation_revision: 1,
			runtime_session_id: RuntimeSessionId::new(RUNTIME_SESSION_ID)
				.expect("RuntimeSession ID"),
			expected_runtime_session_revision: 7,
			user_turn_id: TurnId::new(TURN_ID).expect("Turn ID"),
			expected_user_turn_revision: 1,
			attempt_id: ProviderAttemptId::new(ATTEMPT_ID).expect("ProviderAttempt ID"),
			expected_attempt_revision: 1,
			process_generation_id: ProcessGenerationId::new(GENERATION_ID)
				.expect("ProcessGeneration ID"),
			history_item_id: HistoryItemId::new(INTERRUPTION_HISTORY_ID).expect("HistoryItem ID"),
		};
		let command = CommandIdentity::new("recover-unknown-fixture", b"exact unknown fixture")
			.expect("recovery command");
		assert_eq!(
			store
				.recover_unknown_quick_task_turn(&command, &request)
				.await
				.expect("reject recovery without death evidence"),
			RecoverUnknownQuickTaskTurnOutcome::Rejected
		);

		record_exact_process_death(&store);
		assert!(
			store
				.read_unknown_quick_task_attempt_for_recovery(&conversation_id)
				.await
				.expect("read dead unknown attempt")
				.expect("unknown attempt remains active")
				.process_generation_is_dead
		);
		let recovered = store
			.recover_unknown_quick_task_turn(&command, &request)
			.await
			.expect("recover exact dead unknown Turn");
		assert!(matches!(
			recovered,
			RecoverUnknownQuickTaskTurnOutcome::Applied(ref readback)
				if readback.turn_revision == 2
		));
		assert!(matches!(
			store
				.recover_unknown_quick_task_turn(&command, &request)
				.await
				.expect("replay exact recovery"),
			RecoverUnknownQuickTaskTurnOutcome::Replayed(ref readback)
				if readback.turn_revision == 2
		));
		assert_eq!(
			store
				.read_unknown_quick_task_attempt_for_recovery(&conversation_id)
				.await
				.expect("read recovered projection"),
			None
		);
		let persisted: (String, i64, String, String, i64) = store
			.with_connection(|connection| {
				connection
					.query_row(
						"SELECT t.status, t.revision, p.state, h.inline_text,
						 COUNT(*) OVER () FROM turns AS t
						 JOIN provider_attempts AS p ON p.turn_id = t.turn_id
						 JOIN history_items AS h ON h.turn_id = t.turn_id
						 WHERE t.turn_id = ?1 AND h.history_item_id = ?2",
						params![TURN_ID, INTERRUPTION_HISTORY_ID],
						|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
					)
					.map_err(sqlite_error)
			})
			.expect("read recovered evidence");
		assert_eq!(persisted.0, "failed");
		assert_eq!(persisted.1, 2);
		assert_eq!(persisted.2, "unknown");
		assert_eq!(persisted.3, "Previous turn was interrupted. You can continue.");
		assert_eq!(persisted.4, 1);
		let projection = store
			.read_ordinary_task_conversations(Some(&conversation_id), None, 1)
			.await
			.expect("read recovered conversation projection");
		assert!(matches!(
			projection.as_slice(),
			[OrdinaryTaskConversationProjection::Current(row)]
				if !row.has_unknown_provider_attempt && row.active_turn_id.is_none()
		));
	}

	#[tokio::test]
	async fn positive_evidence_with_an_active_turn_is_durably_terminalizable() {
		let directory = tempdir().expect("temporary database directory");
		let store = SqliteStore::open_test(&directory.path().join("decodex.sqlite3"))
			.expect("initialize database");
		seed_provider_less_starting_task(&store).await;
		seed_active_user_turn(&store);
		seed_unknown_provider_attempt(&store);
		let conversation_id = ConversationId::new(CONVERSATION_ID).expect("conversation ID");
		let evidence_id = ProviderEvidenceId::new("64000000-0000-4000-8000-000000000001")
			.expect("provider evidence ID");
		let evidence = ProviderPositiveEvidence::new(
			evidence_id.clone(),
			ProviderAttemptId::new(ATTEMPT_ID).expect("ProviderAttempt ID"),
			ProviderRequestId::new("61000000-0000-4000-8000-000000000001")
				.expect("provider request ID"),
			ProviderEvidenceSource::ExactThreadReadback,
			ProviderTerminalOutcome::Succeeded,
			ProviderRequestKey::new("app-server:test:1").expect("provider request key"),
			None,
			Some("codex-thread-unknown".to_owned()),
			Some("provider-turn-recovered".to_owned()),
			"9".repeat(64),
		)
		.expect("exact thread readback evidence");
		assert!(matches!(
			store
				.record_provider_attempt_positive_evidence(1, &evidence)
				.await
				.expect("record exact provider evidence"),
			ProviderAttemptMutationOutcome::Applied(ref mutation)
				if mutation.revision == 2
		));

		let pending = store
			.read_pending_quick_task_terminalization(&conversation_id)
			.await
			.expect("read pending terminalization")
			.expect("positive evidence remains terminalizable");
		assert_eq!(pending.attempt_revision, 2);
		assert_eq!(pending.provider_outcome, ProviderTerminalOutcome::Succeeded);
		assert_eq!(pending.provider_turn_id, "provider-turn-recovered");
		let terminalization = TerminalizeQuickTaskTurn {
			conversation_id: pending.conversation_id.clone(),
			expected_conversation_revision: pending.conversation_revision,
			runtime_session_id: pending.runtime_session_id.clone(),
			expected_runtime_session_revision: pending.runtime_session_revision,
			user_turn_id: pending.user_turn_id.clone(),
			expected_user_turn_revision: pending.user_turn_revision,
			assistant_turn: None,
			provider_attempt_id: pending.attempt_id.clone(),
			expected_provider_attempt_revision: pending.attempt_revision,
			provider_evidence_id: pending.evidence_id.clone(),
			provider_outcome: pending.provider_outcome,
			provider_thread_id: pending.codex_thread_id.clone(),
			provider_turn_id: pending.provider_turn_id.clone(),
		};
		assert!(matches!(
			store
				.terminalize_quick_task_turn("pending-terminalization", &terminalization)
				.await
				.expect("terminalize from durable evidence"),
			QuickTaskTerminalizationOutcome::Applied(_)
		));
		assert_eq!(
			store
				.read_pending_quick_task_terminalization(&conversation_id)
				.await
				.expect("read terminalized projection"),
			None
		);
	}

	#[tokio::test]
	async fn recovered_unknown_turn_uses_one_persisted_same_account_context_fallback() {
		let directory = tempdir().expect("temporary database directory");
		let canonical = directory.path().canonicalize().expect("canonical temporary root");
		let root = DecodexRoot::new(canonical).expect("typed Decodex root");
		let paths = root.paths();
		let blob_store = BlobStore::open(paths.clone()).expect("open blob store");
		let store = SqliteStore::open(&paths).expect("initialize product database");
		seed_provider_less_starting_task(&store).await;
		seed_active_user_turn(&store);
		seed_unknown_provider_attempt(&store);
		record_exact_process_death(&store);
		let conversation_id = ConversationId::new(CONVERSATION_ID).expect("conversation ID");
		let recovery = RecoverUnknownQuickTaskTurn {
			conversation_id: conversation_id.clone(),
			expected_conversation_revision: 1,
			runtime_session_id: RuntimeSessionId::new(RUNTIME_SESSION_ID)
				.expect("RuntimeSession ID"),
			expected_runtime_session_revision: 7,
			user_turn_id: TurnId::new(TURN_ID).expect("Turn ID"),
			expected_user_turn_revision: 1,
			attempt_id: ProviderAttemptId::new(ATTEMPT_ID).expect("ProviderAttempt ID"),
			expected_attempt_revision: 1,
			process_generation_id: ProcessGenerationId::new(GENERATION_ID)
				.expect("ProcessGeneration ID"),
			history_item_id: HistoryItemId::new(INTERRUPTION_HISTORY_ID).expect("HistoryItem ID"),
		};
		assert!(matches!(
			store
				.recover_unknown_quick_task_turn(
					&CommandIdentity::new("fallback-recover", b"fallback recovery")
						.expect("recovery command"),
					&recovery,
				)
				.await
				.expect("recover unknown predecessor"),
			RecoverUnknownQuickTaskTurnOutcome::Applied(_)
		));
		store
			.with_connection(|connection| {
				connection
					.execute(
						"INSERT INTO turns (
						 turn_id, conversation_id, runtime_session_id, sequence, role,
						 possible_side_effects, status, revision, created_at_micros,
						 updated_at_micros
						 ) VALUES (?1, ?2, ?3, 2, 'user', 'unknown', 'active', 1, 3, 3)",
						params![SUCCESSOR_TURN_ID, CONVERSATION_ID, RUNTIME_SESSION_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO history_items (
						 history_item_id, conversation_id, turn_id, sequence, kind, role,
						 status, media_type, inline_text, metadata_json, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (?1, ?2, ?3, 2, 'message', 'user', 'completed',
						 'text/markdown', 'Continue safely.', '{}', 1, 3, 3)",
						params![SUCCESSOR_HISTORY_ID, CONVERSATION_ID, SUCCESSOR_TURN_ID],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO routing_decisions (
						 routing_decision_id, operation_id, idempotency_key, request_sha256,
						 authority_shape, conversation_id, turn_id, conversation_revision,
						 source_runtime_session_id, source_runtime_session_revision,
						 account_snapshot_id, profile_snapshot_id, decision_kind, account_id,
						 account_revision, routing_revision, quota_classification, causes_json,
						 exclusions_json, created_at_micros
						 ) VALUES (
						 'a1000000-0000-4000-8000-000000000002',
						 'a2000000-0000-4000-8000-000000000002', 'fallback-route', ?4,
						 'conversation_continuation', ?1, ?2, 1, ?3, 7,
						 '41000000-0000-4000-8000-000000000001',
						 '42000000-0000-4000-8000-000000000001', 'selected', ?5, 1, 1,
						 'known_available', '[]', '[]', 3)",
						params![
							CONVERSATION_ID,
							SUCCESSOR_TURN_ID,
							RUNTIME_SESSION_ID,
							"3".repeat(64),
							ACCOUNT_ID,
						],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("seed successor continuation authority");
		let fallback_history = store
			.recent_conversation_history_excluding_turn(
				&blob_store,
				&conversation_id,
				&TurnId::new(SUCCESSOR_TURN_ID).expect("successor Turn ID"),
				4,
			)
			.await
			.expect("read fallback history before the successor intent");
		assert!(
			fallback_history
				.iter()
				.any(|entry| entry.history_item_id.as_str() == INTERRUPTION_HISTORY_ID)
		);
		assert!(
			fallback_history
				.iter()
				.all(|entry| entry.history_item_id.as_str() != SUCCESSOR_HISTORY_ID)
		);
		let context_pack = compile_context_pack(ContextPackInput {
			conversation_id: conversation_id.clone(),
			possible_side_effects: PossibleSideEffects::Unknown,
			policy: ContextPackPolicy::new(4_096, 4).expect("Context Pack policy"),
			pinned: PinnedContextSource::new(
				"silent-recovery",
				1,
				"The prior provider effect remains unknown. Continue only from this new user intent.",
			)
			.expect("pinned Context Pack source"),
			optional_sources: vec![],
		})
		.expect("compile Context Pack");
		let request = PlanContinuation {
			operation_id: "a5000000-0000-4000-8000-000000000002".to_owned(),
			routing_decision_id: "a1000000-0000-4000-8000-000000000002".to_owned(),
			expected_consumer_revision: 1,
			plan_id: "a6000000-0000-4000-8000-000000000002".to_owned(),
			fallback_runtime_session_id: "a7000000-0000-4000-8000-000000000002".to_owned(),
			fallback_account_snapshot_id: "41000000-0000-4000-8000-000000000001".to_owned(),
			fallback_context_pack_id: "a8000000-0000-4000-8000-000000000002".to_owned(),
		};
		let planned = match store
			.plan_continuation(&blob_store, "fallback-plan", &request, &context_pack)
			.await
			.expect("plan recovered continuation")
		{
			ContinuationCommandOutcome::Success(effect) => effect,
			ContinuationCommandOutcome::Rejected(rejection) => {
				panic!("recovered continuation was rejected: {rejection:?}")
			},
		};
		assert_eq!(planned.plan.kind, ContinuationPlanKind::ContextPackFallback);
		assert_eq!(
			planned.uncertain_predecessor_attempt_id.as_ref().map(ProviderAttemptId::as_str),
			Some(ATTEMPT_ID)
		);
		assert_eq!(
			planned
				.runtime_session
				.as_ref()
				.map(|session| session.account_snapshot.source_account_id.as_str()),
			Some(ACCOUNT_ID)
		);
		assert_eq!(
			planned.fallback_context_pack.as_ref().map(|record| record.pack.digest()),
			Some(context_pack.digest())
		);
		let ownership: (String, i64, String, i64, String) = store
			.with_connection(|connection| {
				connection
					.query_row(
						"SELECT source.state, source.revision, fallback.state,
						 fallback.revision, turn.runtime_session_id
						 FROM runtime_sessions AS source
						 JOIN runtime_sessions AS fallback ON fallback.runtime_session_id = ?2
						 JOIN turns AS turn ON turn.turn_id = ?3
						 WHERE source.runtime_session_id = ?1",
						params![
							RUNTIME_SESSION_ID,
							request.fallback_runtime_session_id,
							SUCCESSOR_TURN_ID,
						],
						|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
					)
					.map_err(sqlite_error)
			})
			.expect("read fallback ownership");
		assert_eq!(ownership.0, "ended");
		assert_eq!(ownership.1, 8);
		assert_eq!(ownership.2, "starting");
		assert_eq!(ownership.3, 1);
		assert_eq!(ownership.4, request.fallback_runtime_session_id);
		store
			.with_connection(|connection| {
				connection
					.execute(
						"UPDATE runtime_sessions SET codex_thread_id = 'fallback-codex-thread',
						 state = 'active', thread_start_request_id = 2,
						 thread_start_request_sha256 = ?2, thread_start_response_id = 2,
						 thread_start_response_sha256 = ?2, revision = 3
						 WHERE runtime_session_id = ?1 AND state = 'starting' AND revision = 1",
						params![request.fallback_runtime_session_id, "4".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO process_execution_epochs (
						 execution_epoch_id, authorization_sha256, created_at_micros
						 ) VALUES ('73000000-0000-4000-8000-000000000002', ?1, 4)",
						params!["5".repeat(64)],
					)
					.map_err(sqlite_error)?;
				connection
					.execute(
						"INSERT INTO process_generations (
						 generation_id, account_id, runtime_session_id, execution_epoch_id,
						 runner_identity, intended_boot_id, control_kind, isolation_kind,
						 bound_boot_id, process_id, process_start_id, process_group_id, session_id,
						 account_revision, credential_schema_version, credential_version,
						 credential_fingerprint, credential_writer_operation_id, provider,
						 provider_account_id, refresh_callback_profile_sha256, state, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (
						 '72000000-0000-4000-8000-000000000002', ?1, ?2,
						 '73000000-0000-4000-8000-000000000002', 'test-runner', 'test-boot',
						 'stdio_only_best_effort_eof', 'session', 'test-boot', 44,
						 'fallback-process', 44, 44, 1, 1, 1, ?3,
						 '11000000-0000-4000-8000-000000000001', 'chatgpt',
						 'local-fixture-provider', ?3, 'ready', 1, 4, 4)",
						params![ACCOUNT_ID, request.fallback_runtime_session_id, "6".repeat(64),],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("activate fallback execution authority");
		let successor_attempt_id = ProviderAttemptId::new("62000000-0000-4000-8000-000000000002")
			.expect("successor attempt ID");
		let successor_request_id = ProviderRequestId::new("63000000-0000-4000-8000-000000000002")
			.expect("successor request ID");
		let provider_keys = ProviderRequestKeys::new(
			None,
			Some(ProviderRequestKey::new("app-server:fallback:1").expect("provider key")),
		)
		.expect("provider keys");
		let preparation = |risk| {
			ProviderAttemptPreparation::new(
				successor_attempt_id.clone(),
				ProviderAttemptConsumer::ConversationTurn {
					conversation_id: conversation_id.clone(),
					turn_id: TurnId::new(SUCCESSOR_TURN_ID).expect("successor Turn ID"),
				},
				request.plan_id.clone(),
				successor_request_id.clone(),
				"7".repeat(64),
				provider_keys.clone(),
				risk,
			)
			.expect("successor ProviderAttempt preparation")
		};
		let fallback_generation_id =
			ProcessGenerationId::new("72000000-0000-4000-8000-000000000002")
				.expect("fallback generation ID");
		let fallback_epoch_id =
			ProcessExecutionEpochId::new("73000000-0000-4000-8000-000000000002")
				.expect("fallback execution epoch ID");
		assert!(matches!(
			store
				.prepare_provider_attempt(
					&preparation(ProviderDuplicateRisk::OriginalIntent),
					&fallback_generation_id,
					1,
					&fallback_epoch_id,
					None,
					(Some(1), Some(1)),
				)
				.await
				.expect("reject unacknowledged fallback attempt"),
			PrepareProviderAttemptOutcome::Rejected { .. }
		));
		let predecessor_attempt_id =
			ProviderAttemptId::new(ATTEMPT_ID).expect("predecessor attempt ID");
		assert!(matches!(
			store
				.prepare_provider_attempt(
					&preparation(ProviderDuplicateRisk::AcknowledgedSuccessor {
						predecessor_attempt_id: predecessor_attempt_id.clone(),
						acknowledgement_digest: "8".repeat(64),
					}),
					&fallback_generation_id,
					1,
					&fallback_epoch_id,
					None,
					(Some(1), Some(1)),
				)
				.await
				.expect("reject forged fallback acknowledgement"),
			PrepareProviderAttemptOutcome::Rejected { .. }
		));
		let acknowledgement_digest = crate::runtime_sessions::digest(&[
			"silent-recovery-successor",
			predecessor_attempt_id.as_str(),
			SUCCESSOR_TURN_ID,
			&request.plan_id,
		]);
		let prepared = store
			.prepare_provider_attempt(
				&preparation(ProviderDuplicateRisk::AcknowledgedSuccessor {
					predecessor_attempt_id: predecessor_attempt_id.clone(),
					acknowledgement_digest,
				}),
				&fallback_generation_id,
				1,
				&fallback_epoch_id,
				None,
				(Some(1), Some(1)),
			)
			.await
			.expect("prepare acknowledged fallback attempt");
		assert!(matches!(prepared, PrepareProviderAttemptOutcome::Fresh(_)));
		let stored_successor = store
			.read_provider_attempt(&successor_attempt_id)
			.await
			.expect("read successor attempt")
			.expect("successor attempt exists");
		assert_eq!(stored_successor.state, ProviderAttemptState::Prepared);
		assert_eq!(
			stored_successor.duplicate_risk,
			ProviderDuplicateRisk::AcknowledgedSuccessor {
				predecessor_attempt_id,
				acknowledgement_digest: crate::runtime_sessions::digest(&[
					"silent-recovery-successor",
					ATTEMPT_ID,
					SUCCESSOR_TURN_ID,
					&request.plan_id,
				]),
			}
		);

		drop(store);
		let reopened = SqliteStore::open(&paths).expect("reopen product database");
		let replayed = match reopened
			.plan_continuation(&blob_store, "fallback-plan", &request, &context_pack)
			.await
			.expect("replay persisted fallback")
		{
			ContinuationCommandOutcome::Success(effect) => effect,
			ContinuationCommandOutcome::Rejected(rejection) => {
				panic!("persisted fallback replay was rejected: {rejection:?}")
			},
		};
		assert_eq!(replayed.plan.kind, ContinuationPlanKind::ContextPackFallback);
		assert_eq!(
			replayed.fallback_context_pack.expect("replayed Context Pack").pack.digest(),
			context_pack.digest()
		);
	}

	#[tokio::test]
	async fn local_archive_rejects_an_active_turn_then_closes_the_safe_projection() {
		let directory = tempdir().expect("temporary database directory");
		let store = SqliteStore::open_test(&directory.path().join("decodex.sqlite3"))
			.expect("initialize database");
		seed_provider_less_starting_task(&store).await;
		seed_active_user_turn(&store);
		let archive = ArchiveLocalQuickTaskConversation {
			conversation_id: ConversationId::new(CONVERSATION_ID).expect("conversation ID"),
			expected_conversation_revision: 1,
			runtime_session_id: RuntimeSessionId::new(RUNTIME_SESSION_ID)
				.expect("RuntimeSession ID"),
			expected_runtime_session_revision: 3,
		};
		assert_eq!(
			store
				.archive_local_quick_task_conversation(
					&CommandIdentity::new("unsafe-local-archive", b"active turn")
						.expect("unsafe archive command"),
					&archive,
				)
				.await
				.expect("reject unsafe local archive"),
			ArchiveLocalQuickTaskConversationOutcome::Rejected
		);
		store
			.with_connection(|connection| {
				connection
					.execute(
						"UPDATE turns SET status = 'failed', revision = 2,
						 completed_at_micros = 2, updated_at_micros = 2 WHERE turn_id = ?1",
						params![TURN_ID],
					)
					.map_err(sqlite_error)?;
				Ok(())
			})
			.expect("make the local projection safe");
		let command = CommandIdentity::new("safe-local-archive", b"no active owner")
			.expect("safe archive command");
		let archived = match store
			.archive_local_quick_task_conversation(&command, &archive)
			.await
			.expect("archive safe local projection")
		{
			ArchiveLocalQuickTaskConversationOutcome::Applied(archived) => archived,
			other => panic!("local archive was not applied: {other:?}"),
		};
		assert_eq!(archived.conversation_revision, 2);
		assert!(matches!(
			store
				.archive_local_quick_task_conversation(&command, &archive)
				.await
				.expect("replay local archive"),
			ArchiveLocalQuickTaskConversationOutcome::Replayed(ref replayed)
				if replayed == &archived
		));
		let states: (String, String) = store
			.with_connection(|connection| {
				connection
					.query_row(
						"SELECT c.state, s.state FROM conversations AS c
						 JOIN runtime_sessions AS s ON s.conversation_id = c.conversation_id
						 WHERE c.conversation_id = ?1",
						params![CONVERSATION_ID],
						|row| Ok((row.get(0)?, row.get(1)?)),
					)
					.map_err(sqlite_error)
			})
			.expect("read archived local projection");
		assert_eq!(states, ("archived".to_owned(), "ended".to_owned()));
	}
}
