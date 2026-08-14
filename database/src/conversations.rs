//! Ordinary Quick Task conversations, turns, and normalized history.

use decodex_core::{
	ArtifactId, BlobHash, BlobStore, ConversationId, HistoryItemId, HistoryItemKind,
	HistoryMediaType, HistoryMetadata, ItemStatus, MAX_BLOB_BYTES, MAX_CONTEXT_RECENT_ITEMS,
	MAX_INLINE_HISTORY_BYTES, PossibleSideEffects, ProviderAttemptId, ProviderEvidenceId,
	ProviderTerminalOutcome, RuntimeSessionId, RuntimeSessionState, TurnId, TurnRole, TurnStatus,
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

/// Create one ordinary Quick Task conversation and retain its original request.
#[derive(Clone, Debug)]
pub struct CreateQuickTaskConversation {
	pub conversation_id: ConversationId,
	pub title: String,
	pub message: String,
	pub working_directory: String,
}

/// Immutable original request coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskRequest {
	pub message: String,
	pub working_directory: String,
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
				   message, working_directory, created_at_micros
				 ) VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
					params![
						create.conversation_id.as_str(),
						command.key,
						initial_turn_id,
						create.message,
						create.working_directory,
						now
					],
				)
				.map_err(sql_error)?;
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
					"SELECT q.message, q.working_directory
				 FROM quick_task_requests AS q
				 JOIN conversations AS c USING (conversation_id)
				 WHERE q.conversation_id = ?1 AND c.state = 'active'",
					params![conversation_id.as_str()],
					|row| {
						Ok(QuickTaskRequest {
							message: row.get(0)?,
							working_directory: row.get(1)?,
						})
					},
				)
				.optional()
				.map_err(sql_error)
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
				"SELECT c.title, q.message, q.working_directory, d.routing_decision_id,
				        d.decision_kind, c.revision
				 FROM conversations AS c
				 JOIN quick_task_requests AS q USING (conversation_id)
				 JOIN routing_decisions AS d ON d.conversation_id = c.conversation_id
				  AND d.authority_shape = 'conversation_account_registry'
				 WHERE c.conversation_id = ?1 AND c.state = 'active'",
				params![request.source_conversation_id.as_str()],
				|row| Ok((
					row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
					row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?,
				)),
			).optional().map_err(sql_error)?;
			let Some((title, message, working_directory, routing_decision_id, decision_kind, revision)) = source else {
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
				 message, working_directory, created_at_micros
				 ) VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7)",
				params![successor_id, key, request.source_conversation_id.as_str(), initial_turn_id, message, working_directory, now],
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
	{
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
			.map_err(sql_error)?
			.ok_or_else(|| incompatible("routing successor redirect"))?;
		return Ok(OrdinaryTaskConversationProjection::RoutingSuccessorRedirect {
			source_conversation_id: conversation_id,
			source_revision: row.2,
			successor_conversation_id: ConversationId::new(successor.0)
				.map_err(|_| incompatible("routing successor identity"))?,
			successor_conversation_revision: successor.1,
		});
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
	let active_turn = connection.query_row(
		"SELECT turn_id FROM turns WHERE conversation_id = ?1 AND role = 'user' AND status = 'active'
		 ORDER BY sequence DESC LIMIT 1",
		params![conversation_id.as_str()], |row| row.get::<_, String>(0),
	).optional().map_err(sql_error)?;
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
	let has_unknown_provider_attempt: bool = connection.query_row(
		"SELECT EXISTS (SELECT 1 FROM provider_attempts WHERE conversation_id = ?1 AND state = 'unknown')",
		params![conversation_id.as_str()], |row| row.get(0),
	).map_err(sql_error)?;
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
			.map(TurnId::new)
			.transpose()
			.map_err(|_| incompatible("active Turn identity"))?,
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
		if limit == 0 || usize::from(limit) > MAX_CONTEXT_RECENT_ITEMS {
			return Err(StoreError::InvalidInput("recent history bound is invalid"));
		}
		let conversation_id = conversation_id.clone();
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
				   WHERE h.conversation_id = ?1 ORDER BY h.sequence DESC LIMIT ?2
				 ) ORDER BY sequence",
					)
					.map_err(sql_error)?;
				let rows = statement
					.query_map(
						params![conversation_id.as_str(), i64::from(limit)],
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
					Some(_) =>
						return Err(StoreError::InvalidInput("history revision must be positive")),
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
