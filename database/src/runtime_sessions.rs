//! RuntimeSession thread-establishment and restart readback authority.

use decodex_core::{
	AccountId, AccountState, ConversationId, ProcessExecutionEpochId, ProcessGenerationId,
	RuntimeSessionId, RuntimeSessionState, TurnId,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

use crate::{
	RoleProfileRole, SqliteStore, StoreError,
	account_lifecycle::{parse_account_state, sql_error},
	unix_micros,
};

/// Exact Conversation, RuntimeSession, and active revision-one Turn coordinates before spawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareQuickTaskProcessGeneration {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
	pub turn_id: TurnId,
	pub expected_turn_revision: i64,
	pub continuation_plan_id: String,
	pub routing_decision_id: String,
	pub selected_account_id: AccountId,
	pub process_generation_id: ProcessGenerationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskProcessGenerationReadback {
	pub request: PrepareQuickTaskProcessGeneration,
	pub admission_revision: Option<i64>,
	pub rejection: Option<QuickTaskProcessGenerationRejection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskProcessGenerationRejection {
	MissingTurn,
	InactiveTurn,
	StaleTurn,
	AuthorityUnavailable,
	InvalidInput,
}

/// One-use pre-spawn admission. Replays cannot construct this type.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshQuickTaskProcessGeneration {
	idempotency_key: String,
	request_sha256: String,
	readback: QuickTaskProcessGenerationReadback,
}

impl FreshQuickTaskProcessGeneration {
	pub const fn readback(&self) -> &QuickTaskProcessGenerationReadback {
		&self.readback
	}

	pub fn generation_id(&self) -> &ProcessGenerationId {
		&self.readback.request.process_generation_id
	}

	pub(crate) fn idempotency_key(&self) -> &str {
		&self.idempotency_key
	}

	pub(crate) fn request_sha256(&self) -> &str {
		&self.request_sha256
	}
}

#[derive(Debug, Eq, PartialEq)]
pub enum PrepareQuickTaskProcessGenerationOutcome {
	Fresh(FreshQuickTaskProcessGeneration),
	Replayed(QuickTaskProcessGenerationReadback),
	Rejected(QuickTaskProcessGenerationReadback),
	Unknown(QuickTaskProcessGenerationReadback),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileQuickTaskThreadEstablishment {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_runtime_session_revision: i64,
	pub turn_id: TurnId,
	pub expected_turn_revision: i64,
	pub continuation_plan_id: String,
	pub routing_decision_id: String,
	pub selected_account_id: AccountId,
	pub process_generation_id: ProcessGenerationId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuickTaskPreEffectEvidenceKind {
	AdmissionRejected,
	SpawnNotCreated,
	ProcessDead,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskThreadStartNonEffect {
	pub process_generation_revision: Option<i64>,
	pub kind: QuickTaskPreEffectEvidenceKind,
	pub evidence_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskThreadEstablishmentReadback {
	Bound(RuntimeSessionThreadBindingReadback),
	Fenced(RuntimeSessionThreadFenceReadback),
	DefinitelyNotStarted(QuickTaskThreadStartNonEffect),
	Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionAccountSnapshot {
	pub account_snapshot_id: String,
	pub source_account_id: AccountId,
	pub display_label: String,
	pub observed_state: AccountState,
	pub source_revision: i64,
	pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionProfileSnapshot {
	pub profile_snapshot_id: String,
	pub role: RoleProfileRole,
	pub source_revision: i64,
	pub model: String,
	pub reasoning_effort: String,
	pub service_tier: String,
	pub instructions_digest: String,
	pub instructions: String,
	pub provenance: Option<String>,
	pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRuntimeSession {
	pub runtime_session_id: RuntimeSessionId,
	pub conversation_id: ConversationId,
	pub profile_snapshot: RuntimeSessionProfileSnapshot,
	pub account_snapshot: RuntimeSessionAccountSnapshot,
	pub codex_thread_id: Option<String>,
	pub last_known_turn_id: Option<String>,
	pub state: RuntimeSessionState,
	pub revision: i64,
	pub created_at: String,
	pub updated_at: String,
	pub ended_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FenceRuntimeSessionThreadStart {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_revision: i64,
	pub turn_id: TurnId,
	pub expected_turn_revision: i64,
	pub continuation_plan_id: String,
	pub process_generation_id: ProcessGenerationId,
	pub process_generation_revision: i64,
	pub process_execution_epoch_id: ProcessExecutionEpochId,
	pub thread_start_request_id: i64,
	pub thread_start_request_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionThreadFenceReadback {
	pub fence_idempotency_key: String,
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub prior_revision: i64,
	pub revision: i64,
	pub turn_id: TurnId,
	pub turn_revision: i64,
	pub continuation_plan_id: String,
	pub routing_decision_id: String,
	pub selected_account_id: AccountId,
	pub process_generation_id: ProcessGenerationId,
	pub process_generation_revision: i64,
	pub process_execution_epoch_id: ProcessExecutionEpochId,
	pub thread_start_request_id: i64,
	pub thread_start_request_sha256: String,
	pub activity_sequence: i64,
	pub outbox_id: i64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct FreshRuntimeSessionThreadStart {
	readback: RuntimeSessionThreadFenceReadback,
}

impl FreshRuntimeSessionThreadStart {
	pub const fn readback(&self) -> &RuntimeSessionThreadFenceReadback {
		&self.readback
	}

	pub fn into_binding(
		self,
		successful_response: SuccessfulRuntimeSessionThreadStart,
	) -> BindRuntimeSessionThread {
		BindRuntimeSessionThread {
			conversation_id: self.readback.conversation_id,
			expected_conversation_revision: self.readback.conversation_revision,
			runtime_session_id: self.readback.runtime_session_id,
			expected_revision: self.readback.revision,
			turn_id: self.readback.turn_id,
			expected_turn_revision: self.readback.turn_revision,
			continuation_plan_id: self.readback.continuation_plan_id,
			fence_idempotency_key: self.readback.fence_idempotency_key,
			thread_start_request_id: self.readback.thread_start_request_id,
			thread_start_request_sha256: self.readback.thread_start_request_sha256,
			successful_response,
		}
	}
}

#[derive(Debug, Eq, PartialEq)]
pub enum FenceRuntimeSessionThreadStartOutcome {
	Fresh(FreshRuntimeSessionThreadStart),
	Replayed(RuntimeSessionThreadFenceReadback),
	Rejected(RuntimeSessionThreadEstablishmentRejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessfulRuntimeSessionThreadStart {
	pub response_id: i64,
	pub response_sha256: String,
	pub codex_thread_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeSessionThreadEstablishmentRejection {
	InvalidInput,
	AuthorityUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindRuntimeSessionThread {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub expected_revision: i64,
	pub turn_id: TurnId,
	pub expected_turn_revision: i64,
	pub continuation_plan_id: String,
	pub fence_idempotency_key: String,
	pub thread_start_request_id: i64,
	pub thread_start_request_sha256: String,
	pub successful_response: SuccessfulRuntimeSessionThreadStart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionThreadBindingReadback {
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub prior_revision: i64,
	pub revision: i64,
	pub turn_id: TurnId,
	pub turn_revision: i64,
	pub fence_prior_revision: i64,
	pub fence_revision: i64,
	pub continuation_plan_id: String,
	pub fence_idempotency_key: String,
	pub binding_idempotency_key: String,
	pub thread_start_request_id: i64,
	pub thread_start_request_sha256: String,
	pub thread_start_response_id: i64,
	pub thread_start_response_sha256: String,
	pub codex_thread_id: String,
	pub activity_sequence: i64,
	pub outbox_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindRuntimeSessionThreadOutcome {
	Applied(RuntimeSessionThreadBindingReadback),
	Replayed(RuntimeSessionThreadBindingReadback),
	Rejected(RuntimeSessionThreadEstablishmentRejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryRuntimeSessionResumeReadback {
	pub conversation_id: ConversationId,
	pub conversation_revision: i64,
	pub runtime_session_id: RuntimeSessionId,
	pub runtime_session_revision: i64,
	pub codex_thread_id: String,
	pub model: String,
	pub reasoning_effort: String,
	pub instructions: String,
	pub source_account_id: AccountId,
	pub source_account_revision: i64,
	pub next_turn_sequence: i64,
	pub thread_start_request_id: i64,
	pub thread_start_request_sha256: String,
	pub thread_start_response_id: i64,
	pub thread_start_response_sha256: String,
	pub has_acknowledged_turn: bool,
	pub has_active_turn: bool,
	pub active_turn_id: Option<TurnId>,
	pub active_turn_revision: Option<i64>,
	pub has_unresolved_provider_attempt: bool,
	pub has_unresolved_process_generation: bool,
}

impl SqliteStore {
	pub async fn prepare_quick_task_process_generation(
		&self,
		idempotency_key: &str,
		request: &PrepareQuickTaskProcessGeneration,
	) -> Result<PrepareQuickTaskProcessGenerationOutcome, StoreError> {
		validate_key(idempotency_key)?;
		let key = idempotency_key.to_owned();
		let request = request.clone();
		let request_sha256 = process_admission_sha(&request);
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(stored_sha) = transaction
				.query_row(
					"SELECT request_sha256 FROM runtime_command_receipts WHERE idempotency_key = ?1",
					params![key],
					|row| row.get::<_, String>(0),
				)
				.optional()
				.map_err(sql_error)?
			{
				if stored_sha != request_sha256 {
					return Err(StoreError::IdempotencyConflict);
				}
				let readback = QuickTaskProcessGenerationReadback {
					request,
					admission_revision: Some(1),
					rejection: None,
				};
				transaction.commit().map_err(sql_error)?;
				return Ok(PrepareQuickTaskProcessGenerationOutcome::Replayed(readback));
			}
			let authority = process_admission_authority(&transaction, &request)?;
			if let Some(rejection) = authority {
				return Ok(PrepareQuickTaskProcessGenerationOutcome::Rejected(
					QuickTaskProcessGenerationReadback {
						request,
						admission_revision: None,
						rejection: Some(rejection),
					},
				));
			}
			let now = unix_micros().map_err(StoreError::from)?;
			transaction
				.execute(
					"INSERT INTO runtime_command_receipts (
				   idempotency_key, request_sha256, operation, entity_id, response_json,
				   completed_at_micros
				 ) VALUES (?1, ?2, 'prepare_quick_task_process_generation', ?3, '{}', ?4)",
					params![key, request_sha256, request.process_generation_id.as_str(), now],
				)
				.map_err(sql_error)?;
			let readback = QuickTaskProcessGenerationReadback {
				request,
				admission_revision: Some(1),
				rejection: None,
			};
			transaction.commit().map_err(sql_error)?;
			Ok(PrepareQuickTaskProcessGenerationOutcome::Fresh(FreshQuickTaskProcessGeneration {
				idempotency_key: key,
				request_sha256,
				readback,
			}))
		})
		.await
	}

	pub async fn fence_runtime_session_thread_start(
		&self,
		idempotency_key: &str,
		fence: &FenceRuntimeSessionThreadStart,
	) -> Result<FenceRuntimeSessionThreadStartOutcome, StoreError> {
		validate_key(idempotency_key)?;
		validate_sha(&fence.thread_start_request_sha256)?;
		if fence.thread_start_request_id <= 0
			|| fence.expected_revision <= 0
			|| fence.expected_turn_revision != 1
			|| fence.process_generation_revision <= 0
		{
			return Err(StoreError::InvalidInput("RuntimeSession thread fence is invalid"));
		}
		let key = idempotency_key.to_owned();
		let fence = fence.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(existing_key) = transaction
				.query_row(
					"SELECT thread_start_fence_key FROM runtime_sessions WHERE runtime_session_id = ?1",
					params![fence.runtime_session_id.as_str()],
					|row| row.get::<_, Option<String>>(0),
				)
				.optional()
				.map_err(sql_error)?
				.flatten()
			{
				let readback = read_thread_fence(&transaction, &fence.runtime_session_id)?;
				transaction.commit().map_err(sql_error)?;
				return if existing_key == key && fence_matches(&readback, &fence) {
					Ok(FenceRuntimeSessionThreadStartOutcome::Replayed(readback))
				} else {
					Ok(FenceRuntimeSessionThreadStartOutcome::Rejected(
						RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable,
					))
				};
			}
			let route = thread_fence_authority(&transaction, &fence)?;
			let Some((routing_decision_id, selected_account_id)) = route else {
				return Ok(FenceRuntimeSessionThreadStartOutcome::Rejected(
					RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable,
				));
			};
			let now = unix_micros().map_err(StoreError::from)?;
			let revision = fence
				.expected_revision
				.checked_add(1)
				.ok_or(StoreError::InvalidInput("RuntimeSession revision overflow"))?;
			let changed = transaction
				.execute(
					"UPDATE runtime_sessions SET
				   thread_start_request_id = ?1, thread_start_request_sha256 = ?2,
				   thread_start_fence_key = ?3, thread_start_turn_id = ?4,
				   thread_start_continuation_plan_id = ?5,
				   thread_start_routing_decision_id = ?6,
				   thread_start_process_generation_id = ?7,
				   thread_start_process_generation_revision = ?8,
				   thread_start_execution_epoch_id = ?9,
				   revision = ?10, updated_at_micros = ?11
				 WHERE runtime_session_id = ?12 AND revision = ?13 AND state = 'starting'",
					params![
						fence.thread_start_request_id,
						fence.thread_start_request_sha256,
						key,
						fence.turn_id.as_str(),
						fence.continuation_plan_id,
						routing_decision_id,
						fence.process_generation_id.as_str(),
						fence.process_generation_revision,
						fence.process_execution_epoch_id.as_str(),
						revision,
						now,
						fence.runtime_session_id.as_str(),
						fence.expected_revision,
					],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Ok(FenceRuntimeSessionThreadStartOutcome::Rejected(
					RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable,
				));
			}
			let readback = RuntimeSessionThreadFenceReadback {
				fence_idempotency_key: key,
				conversation_id: fence.conversation_id,
				conversation_revision: fence.expected_conversation_revision,
				runtime_session_id: fence.runtime_session_id,
				prior_revision: fence.expected_revision,
				revision,
				turn_id: fence.turn_id,
				turn_revision: fence.expected_turn_revision,
				continuation_plan_id: fence.continuation_plan_id,
				routing_decision_id,
				selected_account_id,
				process_generation_id: fence.process_generation_id,
				process_generation_revision: fence.process_generation_revision,
				process_execution_epoch_id: fence.process_execution_epoch_id,
				thread_start_request_id: fence.thread_start_request_id,
				thread_start_request_sha256: fence.thread_start_request_sha256,
				activity_sequence: revision,
				outbox_id: revision,
			};
			transaction.commit().map_err(sql_error)?;
			Ok(FenceRuntimeSessionThreadStartOutcome::Fresh(FreshRuntimeSessionThreadStart {
				readback,
			}))
		})
		.await
	}

	pub async fn bind_runtime_session_thread(
		&self,
		idempotency_key: &str,
		binding: &BindRuntimeSessionThread,
	) -> Result<BindRuntimeSessionThreadOutcome, StoreError> {
		validate_key(idempotency_key)?;
		validate_key(&binding.fence_idempotency_key)?;
		validate_sha(&binding.thread_start_request_sha256)?;
		validate_sha(&binding.successful_response.response_sha256)?;
		if binding.successful_response.response_id != binding.thread_start_request_id
			|| binding.successful_response.codex_thread_id.is_empty()
		{
			return Err(StoreError::InvalidInput("RuntimeSession thread binding is invalid"));
		}
		let key = idempotency_key.to_owned();
		let binding = binding.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(existing_key) = transaction
				.query_row(
					"SELECT thread_start_binding_key FROM runtime_sessions WHERE runtime_session_id = ?1",
					params![binding.runtime_session_id.as_str()],
					|row| row.get::<_, Option<String>>(0),
				)
				.optional()
				.map_err(sql_error)?
				.flatten()
			{
				let readback = read_thread_binding(&transaction, &binding.runtime_session_id)?;
				transaction.commit().map_err(sql_error)?;
				return if existing_key == key && binding_matches(&readback, &binding) {
					Ok(BindRuntimeSessionThreadOutcome::Replayed(readback))
				} else {
					Ok(BindRuntimeSessionThreadOutcome::Rejected(
						RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable,
					))
				};
			}
			let fence = read_thread_fence(&transaction, &binding.runtime_session_id)?;
			if !binding_matches_fence(&binding, &fence) {
				return Ok(BindRuntimeSessionThreadOutcome::Rejected(
					RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable,
				));
			}
			let revision = binding
				.expected_revision
				.checked_add(1)
				.ok_or(StoreError::InvalidInput("RuntimeSession revision overflow"))?;
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = transaction
				.execute(
					"UPDATE runtime_sessions SET codex_thread_id = ?1, state = 'active',
				   thread_start_response_id = ?2, thread_start_response_sha256 = ?3,
				   thread_start_binding_key = ?4, revision = ?5, updated_at_micros = ?6
				 WHERE runtime_session_id = ?7 AND revision = ?8 AND state = 'starting'
				   AND thread_start_fence_key = ?9",
					params![
						binding.successful_response.codex_thread_id,
						binding.successful_response.response_id,
						binding.successful_response.response_sha256,
						key,
						revision,
						now,
						binding.runtime_session_id.as_str(),
						binding.expected_revision,
						binding.fence_idempotency_key,
					],
				)
				.map_err(sql_error)?;
			if changed != 1 {
				return Ok(BindRuntimeSessionThreadOutcome::Rejected(
					RuntimeSessionThreadEstablishmentRejection::AuthorityUnavailable,
				));
			}
			let readback = RuntimeSessionThreadBindingReadback {
				conversation_id: binding.conversation_id,
				conversation_revision: binding.expected_conversation_revision,
				runtime_session_id: binding.runtime_session_id,
				prior_revision: binding.expected_revision,
				revision,
				turn_id: binding.turn_id,
				turn_revision: binding.expected_turn_revision,
				fence_prior_revision: fence.prior_revision,
				fence_revision: fence.revision,
				continuation_plan_id: binding.continuation_plan_id,
				fence_idempotency_key: binding.fence_idempotency_key,
				binding_idempotency_key: key,
				thread_start_request_id: binding.thread_start_request_id,
				thread_start_request_sha256: binding.thread_start_request_sha256,
				thread_start_response_id: binding.successful_response.response_id,
				thread_start_response_sha256: binding.successful_response.response_sha256,
				codex_thread_id: binding.successful_response.codex_thread_id,
				activity_sequence: revision,
				outbox_id: revision,
			};
			transaction.commit().map_err(sql_error)?;
			Ok(BindRuntimeSessionThreadOutcome::Applied(readback))
		})
		.await
	}

	pub async fn reconcile_quick_task_thread_establishment(
		&self,
		request: &ReconcileQuickTaskThreadEstablishment,
	) -> Result<QuickTaskThreadEstablishmentReadback, StoreError> {
		let request = request.clone();
		self.run(move |connection| {
			let admission_sha256 = reconciliation_process_admission_sha(&request);
			let state = connection
				.query_row(
					"SELECT thread_start_fence_key, thread_start_binding_key
				 FROM runtime_sessions WHERE runtime_session_id = ?1",
					params![request.runtime_session_id.as_str()],
					|row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?)),
				)
				.optional()
				.map_err(sql_error)?;
			match state {
				Some((Some(_), Some(_))) => Ok(QuickTaskThreadEstablishmentReadback::Bound(
					read_thread_binding(connection, &request.runtime_session_id)?,
				)),
				Some((Some(_), None)) => Ok(QuickTaskThreadEstablishmentReadback::Fenced(
					read_thread_fence(connection, &request.runtime_session_id)?,
				)),
				_ => {
					let generation = connection
						.query_row(
							"SELECT state, revision, death_evidence_id FROM process_generations
							 WHERE generation_id = ?1",
							params![request.process_generation_id.as_str()],
							|row| {
								Ok((
									row.get::<_, String>(0)?,
									row.get::<_, i64>(1)?,
									row.get::<_, Option<String>>(2)?,
								))
							},
						)
						.optional()
						.map_err(sql_error)?;
					if let Some((state, revision, evidence)) = generation {
						return Ok(match (state.as_str(), evidence) {
							("dead", Some(evidence_id)) =>
								QuickTaskThreadEstablishmentReadback::DefinitelyNotStarted(
									QuickTaskThreadStartNonEffect {
										process_generation_revision: Some(revision),
										kind: QuickTaskPreEffectEvidenceKind::ProcessDead,
										evidence_id,
									},
								),
							_ => QuickTaskThreadEstablishmentReadback::Unknown,
						});
					}
					let admission_evidence = connection
						.query_row(
							"SELECT MIN(idempotency_key) FROM runtime_command_receipts
							 WHERE operation = 'prepare_quick_task_process_generation'
							   AND entity_id = ?1 AND request_sha256 = ?2",
							params![request.process_generation_id.as_str(), admission_sha256],
							|row| row.get::<_, Option<String>>(0),
						)
						.map_err(sql_error)?;
					Ok(admission_evidence.map_or(
						QuickTaskThreadEstablishmentReadback::Unknown,
						|evidence_id| {
							QuickTaskThreadEstablishmentReadback::DefinitelyNotStarted(
								QuickTaskThreadStartNonEffect {
									process_generation_revision: None,
									kind: QuickTaskPreEffectEvidenceKind::AdmissionRejected,
									evidence_id,
								},
							)
						},
					))
				},
			}
		})
		.await
	}

	pub async fn read_ordinary_runtime_session_for_resume(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<OrdinaryRuntimeSessionResumeReadback>, StoreError> {
		let conversation_id = conversation_id.clone();
		self.run(move |connection| {
			let row = connection
				.query_row(
					"SELECT c.revision, s.runtime_session_id, s.revision, s.codex_thread_id,
				        s.model, s.reasoning_effort, s.instructions, s.account_id,
				        s.account_revision,
				        COALESCE((SELECT MAX(t.sequence) + 1 FROM turns AS t
				                  WHERE t.conversation_id = c.conversation_id), 1),
				        s.thread_start_request_id, s.thread_start_request_sha256,
				        s.thread_start_response_id, s.thread_start_response_sha256,
				        s.has_acknowledged_turn,
				        (SELECT t.turn_id FROM turns AS t
				         WHERE t.runtime_session_id = s.runtime_session_id AND t.status = 'active'
				         ORDER BY t.sequence DESC LIMIT 1),
				        (SELECT t.revision FROM turns AS t
				         WHERE t.runtime_session_id = s.runtime_session_id AND t.status = 'active'
				         ORDER BY t.sequence DESC LIMIT 1),
				        EXISTS (SELECT 1 FROM provider_attempts AS p
				                JOIN turns AS pending ON pending.turn_id = p.turn_id
				                WHERE p.runtime_session_id = s.runtime_session_id
				                  AND p.state IN ('prepared', 'dispatch_authorized', 'unknown')
				                  AND pending.status = 'active'),
				        EXISTS (SELECT 1 FROM process_generations AS p
				                WHERE p.runtime_session_id = s.runtime_session_id AND p.state <> 'dead')
				 FROM conversations AS c
				 JOIN runtime_sessions AS s ON s.conversation_id = c.conversation_id
				 WHERE c.conversation_id = ?1 AND c.state = 'active' AND s.state = 'active'",
					params![conversation_id.as_str()],
					|row| {
						Ok((
							row.get::<_, i64>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, i64>(2)?,
							row.get::<_, String>(3)?,
							row.get::<_, String>(4)?,
							row.get::<_, String>(5)?,
							row.get::<_, String>(6)?,
							row.get::<_, String>(7)?,
							row.get::<_, i64>(8)?,
							row.get::<_, i64>(9)?,
							row.get::<_, i64>(10)?,
							row.get::<_, String>(11)?,
							row.get::<_, i64>(12)?,
							row.get::<_, String>(13)?,
							row.get::<_, bool>(14)?,
							row.get::<_, Option<String>>(15)?,
							row.get::<_, Option<i64>>(16)?,
							row.get::<_, bool>(17)?,
							row.get::<_, bool>(18)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			row.map(|row| {
				if row.15.is_some() != row.16.is_some()
					|| row.16.is_some_and(|revision| revision <= 0)
				{
					return Err(incompatible("active Turn coordinates"));
				}
				Ok(OrdinaryRuntimeSessionResumeReadback {
					conversation_id,
					conversation_revision: row.0,
					runtime_session_id: RuntimeSessionId::new(row.1)
						.map_err(|_| incompatible("RuntimeSession identity"))?,
					runtime_session_revision: row.2,
					codex_thread_id: row.3,
					model: row.4,
					reasoning_effort: row.5,
					instructions: row.6,
					source_account_id: AccountId::new(row.7)
						.map_err(|_| incompatible("RuntimeSession account"))?,
					source_account_revision: row.8,
					next_turn_sequence: row.9,
					thread_start_request_id: row.10,
					thread_start_request_sha256: row.11,
					thread_start_response_id: row.12,
					thread_start_response_sha256: row.13,
					has_acknowledged_turn: row.14,
					has_active_turn: row.15.is_some(),
					active_turn_id: row
						.15
						.map(TurnId::new)
						.transpose()
						.map_err(|_| incompatible("active Turn identity"))?,
					active_turn_revision: row.16,
					has_unresolved_provider_attempt: row.17,
					has_unresolved_process_generation: row.18,
				})
			})
			.transpose()
		})
		.await
	}
}

fn process_admission_authority(
	transaction: &rusqlite::Transaction<'_>,
	request: &PrepareQuickTaskProcessGeneration,
) -> Result<Option<QuickTaskProcessGenerationRejection>, StoreError> {
	let turn = transaction
		.query_row(
			"SELECT t.status, t.revision,
		        EXISTS (
			          SELECT 1 FROM continuation_plans AS p
			          JOIN routing_decisions AS d ON d.routing_decision_id = p.routing_decision_id
			          JOIN runtime_sessions AS s ON s.runtime_session_id = p.source_runtime_session_id
			          LEFT JOIN runtime_sessions AS fallback
			            ON fallback.runtime_session_id = p.runtime_session_id
			          JOIN conversations AS c ON c.conversation_id = p.conversation_id
				          WHERE p.continuation_plan_id = ?1 AND d.routing_decision_id = ?2
				            AND p.conversation_id = ?3 AND p.turn_id = ?4
				            AND p.selected_account_id = ?7
				            AND c.revision = ?8 AND c.state = 'active'
				            AND (
				              (p.kind = 'initial_thread' AND p.runtime_session_id = s.runtime_session_id
				               AND p.source_runtime_session_id = ?5
				               AND p.source_runtime_session_revision = ?6
				               AND s.revision = ?6 AND s.state = 'starting')
				              OR
				              (p.kind = 'same_thread' AND p.runtime_session_id IS NULL
				               AND p.source_runtime_session_id = ?5
				               AND p.source_runtime_session_revision = ?6
				               AND s.revision = ?6
				               AND s.state = 'active' AND s.has_acknowledged_turn = 1
			               AND p.codex_thread_id = s.codex_thread_id
			               AND EXISTS (
			                 SELECT 1 FROM provider_attempts AS prior
			                 JOIN provider_attempt_positive_evidence AS evidence
			                   ON evidence.attempt_id = prior.attempt_id
			                 WHERE prior.attempt_id = p.same_thread_attempt_id
			                   AND evidence.evidence_id = p.same_thread_evidence_id
			                   AND prior.runtime_session_id = s.runtime_session_id
			                   AND prior.state IN ('succeeded', 'failed_definitive')
				                   AND evidence.provider_thread_id = s.codex_thread_id
				               ))
				              OR
				              (p.kind = 'context_pack_fallback' AND p.runtime_session_id = ?5
				               AND fallback.revision = ?6 AND fallback.state = 'starting'
				               AND fallback.account_id = p.selected_account_id
				               AND s.state = 'ended'
				               AND s.revision = p.source_runtime_session_revision + 1
				               AND EXISTS (SELECT 1 FROM context_packs AS pack
				                           WHERE pack.context_pack_id = p.fallback_context_pack_id
				                             AND pack.conversation_id = p.conversation_id))
				            )
			        )
		 FROM turns AS t WHERE t.turn_id = ?4 AND t.conversation_id = ?3
		   AND t.runtime_session_id = ?5",
			params![
				request.continuation_plan_id,
				request.routing_decision_id,
				request.conversation_id.as_str(),
				request.turn_id.as_str(),
				request.runtime_session_id.as_str(),
				request.expected_runtime_session_revision,
				request.selected_account_id.as_str(),
				request.expected_conversation_revision,
			],
			|row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, bool>(2)?)),
		)
		.optional()
		.map_err(sql_error)?;
	Ok(match turn {
		None => Some(QuickTaskProcessGenerationRejection::MissingTurn),
		Some((status, _, _)) if status != "active" =>
			Some(QuickTaskProcessGenerationRejection::InactiveTurn),
		Some((_, revision, _)) if revision != request.expected_turn_revision =>
			Some(QuickTaskProcessGenerationRejection::StaleTurn),
		Some((_, _, false)) => Some(QuickTaskProcessGenerationRejection::AuthorityUnavailable),
		Some((_, _, true)) => None,
	})
}

fn thread_fence_authority(
	transaction: &rusqlite::Transaction<'_>,
	fence: &FenceRuntimeSessionThreadStart,
) -> Result<Option<(String, AccountId)>, StoreError> {
	transaction
		.query_row(
			"SELECT p.routing_decision_id, p.selected_account_id
		 FROM continuation_plans AS p
		 JOIN routing_decisions AS d ON d.routing_decision_id = p.routing_decision_id
		 JOIN runtime_sessions AS s
		   ON s.runtime_session_id = COALESCE(p.runtime_session_id, p.source_runtime_session_id)
		 JOIN conversations AS c ON c.conversation_id = p.conversation_id
		 JOIN turns AS t ON t.turn_id = p.turn_id
		 JOIN process_generations AS g ON g.generation_id = ?1
		 WHERE p.continuation_plan_id = ?2 AND p.conversation_id = ?3
		   AND COALESCE(p.runtime_session_id, p.source_runtime_session_id) = ?4
		   AND p.kind IN ('initial_thread', 'context_pack_fallback') AND p.turn_id = ?5
		   AND s.revision = ?6 AND s.state = 'starting'
		   AND c.revision = ?7 AND c.state = 'active'
		   AND t.revision = ?8 AND t.status = 'active'
		   AND g.revision = ?9 AND g.state = 'ready' AND g.runtime_session_id = s.runtime_session_id
		   AND g.execution_epoch_id = ?10 AND g.account_id = p.selected_account_id",
			params![
				fence.process_generation_id.as_str(),
				fence.continuation_plan_id,
				fence.conversation_id.as_str(),
				fence.runtime_session_id.as_str(),
				fence.turn_id.as_str(),
				fence.expected_revision,
				fence.expected_conversation_revision,
				fence.expected_turn_revision,
				fence.process_generation_revision,
				fence.process_execution_epoch_id.as_str(),
			],
			|row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
		)
		.optional()
		.map_err(sql_error)?
		.map(|(decision, account)| {
			AccountId::new(account)
				.map(|account| (decision, account))
				.map_err(|_| incompatible("selected account"))
		})
		.transpose()
}

fn read_thread_fence(
	connection: &rusqlite::Connection,
	runtime_session_id: &RuntimeSessionId,
) -> Result<RuntimeSessionThreadFenceReadback, StoreError> {
	let row = connection
		.query_row(
			"SELECT s.thread_start_fence_key, s.conversation_id, c.revision, s.revision,
		        s.thread_start_turn_id, s.thread_start_continuation_plan_id,
		        s.thread_start_routing_decision_id, s.account_id,
		        s.thread_start_process_generation_id,
		        s.thread_start_process_generation_revision, s.thread_start_execution_epoch_id,
		        s.thread_start_request_id, s.thread_start_request_sha256
		 FROM runtime_sessions AS s JOIN conversations AS c USING (conversation_id)
		 WHERE s.runtime_session_id = ?1 AND s.thread_start_fence_key IS NOT NULL",
			params![runtime_session_id.as_str()],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, i64>(2)?,
					row.get::<_, i64>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, String>(6)?,
					row.get::<_, String>(7)?,
					row.get::<_, String>(8)?,
					row.get::<_, i64>(9)?,
					row.get::<_, String>(10)?,
					row.get::<_, i64>(11)?,
					row.get::<_, String>(12)?,
				))
			},
		)
		.map_err(sql_error)?;
	let revision = row.3;
	Ok(RuntimeSessionThreadFenceReadback {
		fence_idempotency_key: row.0,
		conversation_id: ConversationId::new(row.1).map_err(|_| incompatible("Conversation"))?,
		conversation_revision: row.2,
		runtime_session_id: runtime_session_id.clone(),
		prior_revision: revision.checked_sub(1).ok_or_else(|| incompatible("fence revision"))?,
		revision,
		turn_id: TurnId::new(row.4).map_err(|_| incompatible("Turn"))?,
		turn_revision: 1,
		continuation_plan_id: row.5,
		routing_decision_id: row.6,
		selected_account_id: AccountId::new(row.7).map_err(|_| incompatible("account"))?,
		process_generation_id: ProcessGenerationId::new(row.8)
			.map_err(|_| incompatible("ProcessGeneration"))?,
		process_generation_revision: row.9,
		process_execution_epoch_id: ProcessExecutionEpochId::new(row.10)
			.map_err(|_| incompatible("execution epoch"))?,
		thread_start_request_id: row.11,
		thread_start_request_sha256: row.12,
		activity_sequence: revision,
		outbox_id: revision,
	})
}

fn read_thread_binding(
	connection: &rusqlite::Connection,
	runtime_session_id: &RuntimeSessionId,
) -> Result<RuntimeSessionThreadBindingReadback, StoreError> {
	let fence = read_thread_fence(connection, runtime_session_id)?;
	let row = connection
		.query_row(
			"SELECT revision, thread_start_binding_key, thread_start_response_id,
		        thread_start_response_sha256, codex_thread_id
		 FROM runtime_sessions WHERE runtime_session_id = ?1
		   AND thread_start_binding_key IS NOT NULL AND state = 'active'",
			params![runtime_session_id.as_str()],
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
		.map_err(sql_error)?;
	Ok(RuntimeSessionThreadBindingReadback {
		conversation_id: fence.conversation_id,
		conversation_revision: fence.conversation_revision,
		runtime_session_id: runtime_session_id.clone(),
		prior_revision: fence.revision,
		revision: row.0,
		turn_id: fence.turn_id,
		turn_revision: fence.turn_revision,
		fence_prior_revision: fence.prior_revision,
		fence_revision: fence.revision,
		continuation_plan_id: fence.continuation_plan_id,
		fence_idempotency_key: fence.fence_idempotency_key,
		binding_idempotency_key: row.1,
		thread_start_request_id: fence.thread_start_request_id,
		thread_start_request_sha256: fence.thread_start_request_sha256,
		thread_start_response_id: row.2,
		thread_start_response_sha256: row.3,
		codex_thread_id: row.4,
		activity_sequence: row.0,
		outbox_id: row.0,
	})
}

fn fence_matches(
	readback: &RuntimeSessionThreadFenceReadback,
	fence: &FenceRuntimeSessionThreadStart,
) -> bool {
	readback.conversation_id == fence.conversation_id
		&& readback.conversation_revision == fence.expected_conversation_revision
		&& readback.runtime_session_id == fence.runtime_session_id
		&& readback.prior_revision == fence.expected_revision
		&& readback.turn_id == fence.turn_id
		&& readback.turn_revision == fence.expected_turn_revision
		&& readback.continuation_plan_id == fence.continuation_plan_id
		&& readback.process_generation_id == fence.process_generation_id
		&& readback.process_generation_revision == fence.process_generation_revision
		&& readback.process_execution_epoch_id == fence.process_execution_epoch_id
		&& readback.thread_start_request_id == fence.thread_start_request_id
		&& readback.thread_start_request_sha256 == fence.thread_start_request_sha256
}

fn binding_matches(
	readback: &RuntimeSessionThreadBindingReadback,
	binding: &BindRuntimeSessionThread,
) -> bool {
	readback.conversation_id == binding.conversation_id
		&& readback.runtime_session_id == binding.runtime_session_id
		&& readback.prior_revision == binding.expected_revision
		&& readback.turn_id == binding.turn_id
		&& readback.continuation_plan_id == binding.continuation_plan_id
		&& readback.fence_idempotency_key == binding.fence_idempotency_key
		&& readback.thread_start_request_id == binding.thread_start_request_id
		&& readback.codex_thread_id == binding.successful_response.codex_thread_id
}

fn binding_matches_fence(
	binding: &BindRuntimeSessionThread,
	fence: &RuntimeSessionThreadFenceReadback,
) -> bool {
	binding.conversation_id == fence.conversation_id
		&& binding.expected_conversation_revision == fence.conversation_revision
		&& binding.runtime_session_id == fence.runtime_session_id
		&& binding.expected_revision == fence.revision
		&& binding.turn_id == fence.turn_id
		&& binding.expected_turn_revision == fence.turn_revision
		&& binding.continuation_plan_id == fence.continuation_plan_id
		&& binding.fence_idempotency_key == fence.fence_idempotency_key
		&& binding.thread_start_request_id == fence.thread_start_request_id
		&& binding.thread_start_request_sha256 == fence.thread_start_request_sha256
}

pub(crate) fn read_stored_runtime_session(
	connection: &rusqlite::Connection,
	runtime_session_id: &str,
) -> Result<StoredRuntimeSession, StoreError> {
	let row = connection
		.query_row(
			"SELECT runtime_session_id, conversation_id, account_id, account_revision,
		        account_snapshot_id, account_display_label, account_observed_state,
		        profile_snapshot_id, profile_revision, profile_role, model,
		        reasoning_effort, service_tier, instructions_sha256, instructions,
		        profile_provenance, codex_thread_id, last_known_turn_id, state, revision,
		        created_at_micros, updated_at_micros, ended_at_micros
		 FROM runtime_sessions WHERE runtime_session_id = ?1",
			params![runtime_session_id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, i64>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, String>(6)?,
					row.get::<_, String>(7)?,
					row.get::<_, i64>(8)?,
					row.get::<_, String>(9)?,
					row.get::<_, String>(10)?,
					row.get::<_, String>(11)?,
					row.get::<_, String>(12)?,
					row.get::<_, String>(13)?,
					row.get::<_, String>(14)?,
					row.get::<_, Option<String>>(15)?,
					row.get::<_, Option<String>>(16)?,
					row.get::<_, Option<String>>(17)?,
					row.get::<_, String>(18)?,
					row.get::<_, i64>(19)?,
					row.get::<_, i64>(20)?,
					row.get::<_, i64>(21)?,
					row.get::<_, Option<i64>>(22)?,
				))
			},
		)
		.map_err(sql_error)?;
	if row.9 != RoleProfileRole::Task.as_sql() {
		return Err(incompatible("RoleProfile role"));
	}
	let state = match row.18.as_str() {
		"starting" => RuntimeSessionState::Starting,
		"active" => RuntimeSessionState::Active,
		"ended" => RuntimeSessionState::Ended,
		"diverged" => RuntimeSessionState::Diverged,
		_ => return Err(incompatible("RuntimeSession state")),
	};
	Ok(StoredRuntimeSession {
		runtime_session_id: RuntimeSessionId::new(row.0)
			.map_err(|_| incompatible("RuntimeSession identity"))?,
		conversation_id: ConversationId::new(row.1)
			.map_err(|_| incompatible("Conversation identity"))?,
		account_snapshot: RuntimeSessionAccountSnapshot {
			account_snapshot_id: row.4,
			source_account_id: AccountId::new(row.2)
				.map_err(|_| incompatible("account identity"))?,
			display_label: row.5,
			observed_state: parse_account_state(&row.6)?,
			source_revision: row.3,
			created_at: row.20.to_string(),
		},
		profile_snapshot: RuntimeSessionProfileSnapshot {
			profile_snapshot_id: row.7,
			role: RoleProfileRole::Task,
			source_revision: row.8,
			model: row.10,
			reasoning_effort: row.11,
			service_tier: row.12,
			instructions_digest: row.13,
			instructions: row.14,
			provenance: row.15,
			created_at: row.20.to_string(),
		},
		codex_thread_id: row.16,
		last_known_turn_id: row.17,
		state,
		revision: row.19,
		created_at: row.20.to_string(),
		updated_at: row.21.to_string(),
		ended_at: row.22.map(|value| value.to_string()),
	})
}

fn process_admission_sha(request: &PrepareQuickTaskProcessGeneration) -> String {
	digest(&[
		request.conversation_id.as_str(),
		&request.expected_conversation_revision.to_string(),
		request.runtime_session_id.as_str(),
		&request.expected_runtime_session_revision.to_string(),
		request.turn_id.as_str(),
		&request.expected_turn_revision.to_string(),
		&request.continuation_plan_id,
		&request.routing_decision_id,
		request.selected_account_id.as_str(),
		request.process_generation_id.as_str(),
	])
}

fn reconciliation_process_admission_sha(request: &ReconcileQuickTaskThreadEstablishment) -> String {
	digest(&[
		request.conversation_id.as_str(),
		&request.expected_conversation_revision.to_string(),
		request.runtime_session_id.as_str(),
		&request.expected_runtime_session_revision.to_string(),
		request.turn_id.as_str(),
		&request.expected_turn_revision.to_string(),
		&request.continuation_plan_id,
		&request.routing_decision_id,
		request.selected_account_id.as_str(),
		request.process_generation_id.as_str(),
	])
}

pub(crate) fn digest(parts: &[&str]) -> String {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update(part.len().to_be_bytes());
		digest.update(part.as_bytes());
	}
	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_key(key: &str) -> Result<(), StoreError> {
	if key.is_empty() || key.len() > 256 || decodex_core::contains_credential_material(key) {
		return Err(StoreError::InvalidInput("idempotency key is invalid"));
	}
	Ok(())
}

fn validate_sha(value: &str) -> Result<(), StoreError> {
	if value.len() != 64
		|| !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
	{
		return Err(StoreError::InvalidInput("SHA-256 value is invalid"));
	}
	Ok(())
}

fn incompatible(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
