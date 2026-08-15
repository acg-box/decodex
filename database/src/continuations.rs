//! Inert continuation plans over one immutable routing decision.

use decodex_core::{
	AccountId, ArtifactId, BlobHash, BlobStore, ContextPack, ContextPackPolicy,
	ContextSourceDisposition, ContextSourceKind, ContextSourceManifest, ContinuationCommandOutcome,
	ContinuationPlan, ContinuationPlanKind, ContinuationRejection, ConversationId,
	ExecutionConsumer, PossibleSideEffects, ProviderAttemptId, ProviderEvidenceId,
	RuntimeSessionId, SameThreadContinuationEvidence, TurnId,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
	SqliteStore, StoreError, StoredRuntimeSession,
	account_lifecycle::{random_uuid_v4, sql_error},
	runtime_sessions::{digest, read_stored_runtime_session},
	unix_micros,
};

/// Exact coordinates for an existing-session continuation plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanContinuation {
	pub operation_id: String,
	pub routing_decision_id: String,
	pub expected_consumer_revision: i64,
	pub plan_id: String,
	pub fallback_runtime_session_id: String,
	pub fallback_account_snapshot_id: String,
	pub fallback_context_pack_id: String,
}

/// Exact coordinates for creating the first unfenced RuntimeSession and plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanInitialThreadContinuation {
	pub operation_id: String,
	pub routing_decision_id: String,
	pub expected_conversation_revision: i64,
	pub plan_id: String,
}

/// Verified Context Pack metadata. The local slice currently emits no fallback effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPackRecord {
	pub context_pack_id: String,
	pub conversation_id: ConversationId,
	pub pack_revision: i64,
	pub compiled_digest: BlobHash,
	pub byte_length: u64,
	pub truncated: bool,
	pub omitted_source_count: usize,
	pub pack: ContextPack,
}

/// Strict committed continuation-plan readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlanEffect {
	pub plan: ContinuationPlan,
	pub runtime_session: Option<StoredRuntimeSession>,
	pub fallback_context_pack: Option<ContextPackRecord>,
	/// Exact recovered unknown predecessor that requires an acknowledged successor effect.
	pub uncertain_predecessor_attempt_id: Option<ProviderAttemptId>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistedContextSourceManifest {
	kind: ContextSourceKind,
	source_id: String,
	revision: u64,
	content_digest: String,
	original_byte_length: u64,
	included_byte_length: u64,
	included_digest: String,
	disposition: String,
	artifact_id: Option<String>,
	artifact_revision: Option<u64>,
}

struct ExistingContinuationAuthority {
	conversation_id: String,
	turn_id: String,
	source_runtime_session_id: String,
	source_runtime_session_revision: i64,
	account_id: String,
	codex_thread_id: String,
	account_revision: i64,
	account_display_label: String,
	account_observed_state: String,
	credential_binding_json: String,
	profile_revision: i64,
	profile_role: String,
	model: String,
	reasoning_effort: String,
	instructions: String,
	service_tier: String,
	instructions_sha256: String,
	profile_provenance: Option<String>,
	has_acknowledged_turn: bool,
	latest_attempt_id: Option<String>,
	latest_attempt_state: Option<String>,
	latest_evidence_id: Option<String>,
	latest_evidence_thread_id: Option<String>,
	latest_unknown_is_recoverable: bool,
}

impl SqliteStore {
	/// Consume one selected initial decision and create one starting RuntimeSession.
	#[allow(clippy::too_many_lines)] // Keep one atomic continuation-plan transaction together.
	pub async fn plan_initial_thread_continuation(
		&self,
		idempotency_key: &str,
		request: &PlanInitialThreadContinuation,
	) -> Result<ContinuationCommandOutcome<ContinuationPlanEffect>, StoreError> {
		validate_key(idempotency_key)?;
		if request.expected_conversation_revision <= 0 {
			return Err(StoreError::InvalidInput("Conversation revision must be positive"));
		}
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = initial_request_sha(&request);
			if let Some((stored_sha, plan_id)) = transaction
				.query_row(
					"SELECT request_sha256, continuation_plan_id FROM continuation_plans
				 WHERE idempotency_key = ?1",
					params![key],
					|row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
				)
				.optional()
				.map_err(sql_error)?
			{
				if stored_sha != request_sha {
					return Err(StoreError::IdempotencyConflict);
				}
				let effect = read_plan_effect(&transaction, &plan_id, None)?;
				transaction.commit().map_err(sql_error)?;
				return Ok(ContinuationCommandOutcome::Success(effect));
			}

			let authority = transaction
				.query_row(
					"SELECT d.conversation_id, d.turn_id, d.account_id, d.account_revision,
				        a.display_label, a.state, p.revision, p.model, p.reasoning_effort,
				        p.service_tier, p.instructions
				 FROM routing_decisions AS d
				 JOIN conversations AS c ON c.conversation_id = d.conversation_id
				 JOIN accounts AS a ON a.account_id = d.account_id
				 JOIN role_profiles AS p ON p.role = 'task'
				 WHERE d.routing_decision_id = ?1
				   AND d.authority_shape = 'conversation_account_registry'
				   AND d.decision_kind = 'selected' AND c.state = 'active'
				   AND c.revision = ?2 AND d.conversation_revision = ?2
				   AND d.account_revision = a.revision",
					params![request.routing_decision_id, request.expected_conversation_revision],
					|row| {
						Ok((
							row.get::<_, String>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, String>(2)?,
							row.get::<_, i64>(3)?,
							row.get::<_, String>(4)?,
							row.get::<_, String>(5)?,
							row.get::<_, i64>(6)?,
							row.get::<_, String>(7)?,
							row.get::<_, String>(8)?,
							row.get::<_, String>(9)?,
							row.get::<_, String>(10)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some(authority) = authority else {
				return Ok(ContinuationCommandOutcome::Rejected(
					ContinuationRejection::MissingDecision,
				));
			};
			let runtime_session_id = RuntimeSessionId::new(random_uuid_v4()?)
				.map_err(|_| incompatible("generated RuntimeSession identity"))?;
			let account_snapshot_id = random_uuid_v4()?;
			let profile_snapshot_id = random_uuid_v4()?;
			let instructions_sha256 = Sha256::digest(authority.10.as_bytes())
				.iter()
				.map(|byte| format!("{byte:02x}"))
				.collect::<String>();
			let credential_binding_json: String = transaction
				.query_row(
					"SELECT json_object(
				   'schema_version', schema_version, 'credential_version', credential_version,
				   'fingerprint', fingerprint, 'writer_operation_id', writer_operation_id,
				   'provider', provider, 'provider_account_id', provider_account_id
				 ) FROM account_credentials WHERE account_id = ?1",
					params![authority.2],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			let now = unix_micros().map_err(StoreError::from)?;
			transaction
				.execute(
					"INSERT INTO runtime_sessions (
				   runtime_session_id, conversation_id, account_id, account_revision,
				   account_snapshot_id, account_display_label, account_observed_state,
				   credential_binding_json, profile_snapshot_id, profile_revision, profile_role,
				   model, reasoning_effort, instructions, service_tier, instructions_sha256,
				   profile_provenance, state, revision, created_at_micros, updated_at_micros
				 ) VALUES (
				   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'task', ?11, ?12, ?13,
				   ?14, ?15, NULL, 'starting', 1, ?16, ?16
				 )",
					params![
						runtime_session_id.as_str(),
						authority.0,
						authority.2,
						authority.3,
						account_snapshot_id,
						authority.4,
						authority.5,
						credential_binding_json,
						profile_snapshot_id,
						authority.6,
						authority.7,
						authority.8,
						authority.10,
						authority.9,
						instructions_sha256,
						now,
					],
				)
				.map_err(sql_error)?;
			transaction
				.execute(
					"INSERT INTO continuation_plans (
				   continuation_plan_id, operation_id, idempotency_key, request_sha256,
				   conversation_id, turn_id, routing_decision_id, source_runtime_session_id,
				   source_runtime_session_revision, selected_account_id, runtime_session_id,
				   kind, created_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?8, 'initial_thread', ?10)",
					params![
						request.plan_id,
						request.operation_id,
						key,
						request_sha,
						authority.0,
						authority.1,
						request.routing_decision_id,
						runtime_session_id.as_str(),
						authority.2,
						now,
					],
				)
				.map_err(sql_error)?;
			let effect = read_plan_effect(&transaction, &request.plan_id, None)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ContinuationCommandOutcome::Success(effect))
		})
		.await
	}

	/// Plan same-thread continuation from exact terminal evidence, or atomically replace the
	/// RuntimeSession with a same-account Context Pack when a recovered predecessor is uncertain.
	pub async fn plan_continuation(
		&self,
		blob_store: &BlobStore,
		idempotency_key: &str,
		request: &PlanContinuation,
		fallback_pack: &ContextPack,
	) -> Result<ContinuationCommandOutcome<ContinuationPlanEffect>, StoreError> {
		validate_key(idempotency_key)?;
		if request.expected_consumer_revision <= 0 {
			return Err(StoreError::InvalidInput("execution consumer revision must be positive"));
		}
		fallback_pack
			.verify()
			.map_err(|_| StoreError::InvalidInput("fallback Context Pack is invalid"))?;
		let compiled_digest = blob_store.put(fallback_pack.bytes())?;
		if compiled_digest != fallback_pack.digest() {
			return Err(StoreError::Incompatible("Context Pack blob digest differs".to_owned()));
		}
		let manifest_json = serialize_context_manifest(fallback_pack)?;
		let manifest_digest = fallback_pack.manifest_digest().to_hex();
		let byte_length = i64::try_from(fallback_pack.bytes().len())
			.map_err(|_| StoreError::InvalidInput("fallback Context Pack is too large"))?;
		let omitted_source_count = i64::try_from(fallback_pack.omitted_source_count())
			.map_err(|_| StoreError::InvalidInput("fallback Context Pack manifest is too large"))?;
		let blob_store = blob_store.clone();
		let fallback_pack = fallback_pack.clone();
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = continuation_request_sha(&request);
			if let Some((stored_sha, plan_id)) = transaction
				.query_row(
					"SELECT request_sha256, continuation_plan_id FROM continuation_plans
				 WHERE idempotency_key = ?1",
					params![key],
					|row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
				)
				.optional()
				.map_err(sql_error)?
			{
				if stored_sha != request_sha {
					return Err(StoreError::IdempotencyConflict);
				}
				let effect = read_plan_effect(&transaction, &plan_id, Some(&blob_store))?;
				if effect.plan.kind == ContinuationPlanKind::ContextPackFallback
					&& effect
						.fallback_context_pack
						.as_ref()
						.is_none_or(|record| record.pack.digest() != fallback_pack.digest())
				{
					return Err(StoreError::IdempotencyConflict);
				}
				transaction.commit().map_err(sql_error)?;
				return Ok(ContinuationCommandOutcome::Success(effect));
			}
			let authority = transaction
				.query_row(
					"SELECT d.conversation_id, d.turn_id, d.source_runtime_session_id,
				        d.source_runtime_session_revision, d.account_id, s.codex_thread_id,
				        s.account_revision, s.account_display_label,
				        s.account_observed_state, s.credential_binding_json, s.profile_revision,
				        s.profile_role, s.model, s.reasoning_effort, s.instructions,
				        s.service_tier, s.instructions_sha256, s.profile_provenance,
				        s.has_acknowledged_turn, p.attempt_id, p.state,
				        e.evidence_id, e.provider_thread_id,
				        CASE WHEN p.state = 'unknown'
				          AND EXISTS (SELECT 1 FROM turns AS prior_turn
				                      WHERE prior_turn.turn_id = p.turn_id
				                        AND prior_turn.status = 'failed')
					          AND EXISTS (SELECT 1 FROM process_generations AS g
					                      JOIN process_generation_death_evidence AS death
					                        ON death.generation_id = g.generation_id
					                       AND death.evidence_id = g.death_evidence_id
				                      WHERE g.generation_id = p.process_generation_id
				                        AND g.state = 'dead')
				          THEN 1 ELSE 0 END
				 FROM routing_decisions AS d
				 JOIN runtime_sessions AS s ON s.runtime_session_id = d.source_runtime_session_id
				 LEFT JOIN provider_attempts AS p ON p.attempt_id = (
				   SELECT latest.attempt_id FROM provider_attempts AS latest
				   WHERE latest.runtime_session_id = s.runtime_session_id
				   ORDER BY latest.created_at_micros DESC, latest.attempt_id DESC LIMIT 1
				 )
				 LEFT JOIN provider_attempt_positive_evidence AS e ON e.attempt_id = p.attempt_id
				   AND e.evidence_id = p.terminal_evidence_id
				 WHERE d.routing_decision_id = ?1
				   AND d.authority_shape = 'conversation_continuation'
				   AND d.conversation_revision = ?2 AND d.decision_kind = 'selected'
				   AND s.state = 'active' AND s.revision = d.source_runtime_session_revision
				   AND d.account_id = s.account_id
				   AND d.account_snapshot_id = ?3
				   AND d.account_snapshot_id = s.account_snapshot_id
				   AND d.profile_snapshot_id = s.profile_snapshot_id
				   AND EXISTS (SELECT 1 FROM turns AS current_turn
				               WHERE current_turn.turn_id = d.turn_id
				                 AND current_turn.conversation_id = d.conversation_id
				                 AND current_turn.runtime_session_id = s.runtime_session_id
				                 AND current_turn.status = 'active'
				                 AND current_turn.revision = 1)",
					params![
						request.routing_decision_id,
						request.expected_consumer_revision,
						request.fallback_account_snapshot_id,
					],
					|row| {
						Ok(ExistingContinuationAuthority {
							conversation_id: row.get(0)?,
							turn_id: row.get(1)?,
							source_runtime_session_id: row.get(2)?,
							source_runtime_session_revision: row.get(3)?,
							account_id: row.get(4)?,
							codex_thread_id: row.get(5)?,
							account_revision: row.get(6)?,
							account_display_label: row.get(7)?,
							account_observed_state: row.get(8)?,
							credential_binding_json: row.get(9)?,
							profile_revision: row.get(10)?,
							profile_role: row.get(11)?,
							model: row.get(12)?,
							reasoning_effort: row.get(13)?,
							instructions: row.get(14)?,
							service_tier: row.get(15)?,
							instructions_sha256: row.get(16)?,
							profile_provenance: row.get(17)?,
							has_acknowledged_turn: row.get(18)?,
							latest_attempt_id: row.get(19)?,
							latest_attempt_state: row.get(20)?,
							latest_evidence_id: row.get(21)?,
							latest_evidence_thread_id: row.get(22)?,
							latest_unknown_is_recoverable: row.get(23)?,
						})
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some(authority) = authority else {
				return Ok(ContinuationCommandOutcome::Rejected(
					ContinuationRejection::SameThreadUnavailable,
				));
			};
			let same_thread = matches!(
				authority.latest_attempt_state.as_deref(),
				Some("succeeded" | "failed_definitive")
			) && authority.latest_evidence_thread_id.as_deref()
				== Some(authority.codex_thread_id.as_str())
				&& authority.latest_attempt_id.is_some()
				&& authority.latest_evidence_id.is_some();
			let now = unix_micros().map_err(StoreError::from)?;
			if same_thread {
				transaction
					.execute(
						"INSERT INTO continuation_plans (
				   continuation_plan_id, operation_id, idempotency_key, request_sha256,
				   conversation_id, turn_id, routing_decision_id, source_runtime_session_id,
				   source_runtime_session_revision, selected_account_id, runtime_session_id,
				   kind, codex_thread_id, same_thread_attempt_id, same_thread_evidence_id,
				   created_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL,
				           'same_thread', ?11, ?12, ?13, ?14)",
						params![
							request.plan_id,
							request.operation_id,
							key,
							request_sha,
							authority.conversation_id,
							authority.turn_id,
							request.routing_decision_id,
							authority.source_runtime_session_id,
							authority.source_runtime_session_revision,
							authority.account_id,
							authority.codex_thread_id,
							authority.latest_attempt_id,
							authority.latest_evidence_id,
							now,
						],
					)
					.map_err(sql_error)?;
			} else {
				let fallback_allowed = authority.has_acknowledged_turn
					&& match authority.latest_attempt_state.as_deref() {
						Some("unknown") => authority.latest_unknown_is_recoverable,
						Some("not_submitted" | "canceled") => true,
						_ => false,
					};
				if !fallback_allowed
					|| fallback_pack.conversation_id().as_str()
						!= authority.conversation_id.as_str()
					|| fallback_pack.possible_side_effects() != PossibleSideEffects::Unknown
					|| authority.profile_role != "task"
				{
					return Ok(ContinuationCommandOutcome::Rejected(
						ContinuationRejection::SameThreadUnavailable,
					));
				}
				persist_context_pack(
					&transaction,
					&request.fallback_context_pack_id,
					&authority.conversation_id,
					&fallback_pack,
					&manifest_json,
					&manifest_digest,
					compiled_digest,
					byte_length,
					omitted_source_count,
					now,
				)?;
				let source_changed = transaction
					.execute(
						"UPDATE runtime_sessions SET state = 'ended', revision = revision + 1,
						 updated_at_micros = ?3, ended_at_micros = ?3
						 WHERE runtime_session_id = ?1 AND revision = ?2 AND state = 'active'",
						params![
							authority.source_runtime_session_id,
							authority.source_runtime_session_revision,
							now,
						],
					)
					.map_err(sql_error)?;
				if source_changed != 1 {
					return Ok(ContinuationCommandOutcome::Rejected(
						ContinuationRejection::StaleConsumerRevision,
					));
				}
				let account_snapshot_id = random_uuid_v4()?;
				let profile_snapshot_id = random_uuid_v4()?;
				transaction
					.execute(
						"INSERT INTO runtime_sessions (
						 runtime_session_id, conversation_id, account_id, account_revision,
						 account_snapshot_id, account_display_label, account_observed_state,
						 credential_binding_json, profile_snapshot_id, profile_revision,
						 profile_role, model, reasoning_effort, instructions, service_tier,
						 instructions_sha256, profile_provenance, state, revision,
						 created_at_micros, updated_at_micros
						 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'task',
						 ?11, ?12, ?13, ?14, ?15, ?16, 'starting', 1, ?17, ?17)",
						params![
							request.fallback_runtime_session_id,
							authority.conversation_id,
							authority.account_id,
							authority.account_revision,
							account_snapshot_id,
							authority.account_display_label,
							authority.account_observed_state,
							authority.credential_binding_json,
							profile_snapshot_id,
							authority.profile_revision,
							authority.model,
							authority.reasoning_effort,
							authority.instructions,
							authority.service_tier,
							authority.instructions_sha256,
							authority.profile_provenance,
							now,
						],
					)
					.map_err(sql_error)?;
				let turn_changed = transaction
					.execute(
						"UPDATE turns SET runtime_session_id = ?1, updated_at_micros = ?5
						 WHERE turn_id = ?2 AND conversation_id = ?3 AND runtime_session_id = ?4
						   AND status = 'active' AND revision = 1",
						params![
							request.fallback_runtime_session_id,
							authority.turn_id,
							authority.conversation_id,
							authority.source_runtime_session_id,
							now,
						],
					)
					.map_err(sql_error)?;
				if turn_changed != 1 {
					return Ok(ContinuationCommandOutcome::Rejected(
						ContinuationRejection::StaleConsumerRevision,
					));
				}
				transaction
					.execute(
						"INSERT INTO continuation_plans (
						 continuation_plan_id, operation_id, idempotency_key, request_sha256,
						 conversation_id, turn_id, routing_decision_id,
						 source_runtime_session_id, source_runtime_session_revision,
						 selected_account_id, runtime_session_id, kind,
						 fallback_context_pack_id, created_at_micros
						 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
						 'context_pack_fallback', ?12, ?13)",
						params![
							request.plan_id,
							request.operation_id,
							key,
							request_sha,
							authority.conversation_id,
							authority.turn_id,
							request.routing_decision_id,
							authority.source_runtime_session_id,
							authority.source_runtime_session_revision,
							authority.account_id,
							request.fallback_runtime_session_id,
							request.fallback_context_pack_id,
							now,
						],
					)
					.map_err(sql_error)?;
			}
			let effect = read_plan_effect(&transaction, &request.plan_id, Some(&blob_store))?;
			transaction.commit().map_err(sql_error)?;
			Ok(ContinuationCommandOutcome::Success(effect))
		})
		.await
	}
}

fn read_plan_effect(
	connection: &rusqlite::Connection,
	plan_id: &str,
	blob_store: Option<&BlobStore>,
) -> Result<ContinuationPlanEffect, StoreError> {
	let row = connection
		.query_row(
			"SELECT p.operation_id, p.routing_decision_id, p.conversation_id, p.turn_id,
		        p.source_runtime_session_id, p.source_runtime_session_revision,
		        p.selected_account_id, p.runtime_session_id, p.kind, p.codex_thread_id,
		        p.fallback_context_pack_id, p.same_thread_attempt_id,
		        p.same_thread_evidence_id, p.created_at_micros,
		        d.conversation_revision
		 FROM continuation_plans AS p
		 JOIN routing_decisions AS d ON d.routing_decision_id = p.routing_decision_id
		 WHERE p.continuation_plan_id = ?1",
			params![plan_id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, i64>(5)?,
					row.get::<_, String>(6)?,
					row.get::<_, Option<String>>(7)?,
					row.get::<_, String>(8)?,
					row.get::<_, Option<String>>(9)?,
					row.get::<_, Option<String>>(10)?,
					row.get::<_, Option<String>>(11)?,
					row.get::<_, Option<String>>(12)?,
					row.get::<_, i64>(13)?,
					row.get::<_, i64>(14)?,
				))
			},
		)
		.map_err(sql_error)?;
	let conversation_id =
		ConversationId::new(row.2).map_err(|_| incompatible("Conversation identity"))?;
	let turn_id = TurnId::new(row.3).map_err(|_| incompatible("Turn identity"))?;
	let source_runtime_session_id =
		RuntimeSessionId::new(row.4).map_err(|_| incompatible("RuntimeSession identity"))?;
	let selected_account_id =
		AccountId::new(row.6).map_err(|_| incompatible("selected account identity"))?;
	let consumer = ExecutionConsumer::ConversationTurn {
		conversation_id: conversation_id.clone(),
		conversation_revision: row.14,
		source_runtime_session_id: (row.8 != "initial_thread")
			.then(|| source_runtime_session_id.clone()),
		source_runtime_session_revision: (row.8 != "initial_thread").then_some(row.5),
		turn_id,
	};
	let (kind, same_thread_evidence) = match row.8.as_str() {
		"initial_thread" => (ContinuationPlanKind::InitialThread, None),
		"same_thread" => {
			let attempt_id =
				ProviderAttemptId::new(row.11.ok_or_else(|| incompatible("same-thread attempt"))?)
					.map_err(|_| incompatible("same-thread attempt"))?;
			let evidence_id = ProviderEvidenceId::new(
				row.12.ok_or_else(|| incompatible("same-thread evidence"))?,
			)
			.map_err(|_| incompatible("same-thread evidence"))?;
			let attempt_revision: i64 = connection
				.query_row(
					"SELECT revision FROM provider_attempts WHERE attempt_id = ?1",
					params![attempt_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			(
				ContinuationPlanKind::SameThread,
				Some(SameThreadContinuationEvidence::ProviderAttempt {
					attempt_id,
					attempt_revision,
					evidence_id,
				}),
			)
		},
		"context_pack_fallback" => (ContinuationPlanKind::ContextPackFallback, None),
		_ => return Err(incompatible("Continuation Plan kind")),
	};
	let runtime_session =
		row.7.as_deref().map(|id| read_stored_runtime_session(connection, id)).transpose()?;
	let fallback_context_pack = if kind == ContinuationPlanKind::ContextPackFallback {
		let context_pack_id =
			row.10.as_deref().ok_or_else(|| incompatible("fallback Context Pack identity"))?;
		Some(read_context_pack(
			connection,
			blob_store.ok_or_else(|| incompatible("fallback Context Pack blob owner"))?,
			context_pack_id,
			&conversation_id,
		)?)
	} else {
		None
	};
	let uncertain_predecessor_attempt_id = if kind == ContinuationPlanKind::ContextPackFallback {
		connection
			.query_row(
				"SELECT latest.attempt_id FROM provider_attempts AS latest
				 WHERE latest.attempt_id = (
				   SELECT candidate.attempt_id FROM provider_attempts AS candidate
				   WHERE candidate.runtime_session_id = ?1
				   ORDER BY candidate.created_at_micros DESC, candidate.attempt_id DESC LIMIT 1
				 ) AND latest.state = 'unknown'",
				params![source_runtime_session_id.as_str()],
				|row| row.get::<_, String>(0),
			)
			.optional()
			.map_err(sql_error)?
			.map(ProviderAttemptId::new)
			.transpose()
			.map_err(|_| incompatible("fallback predecessor attempt"))?
	} else {
		None
	};
	Ok(ContinuationPlanEffect {
		plan: ContinuationPlan {
			plan_id: plan_id.to_owned(),
			operation_id: row.0,
			routing_decision_id: row.1,
			consumer,
			conversation_id,
			source_runtime_session_id,
			source_runtime_session_revision: row.5,
			selected_account_id,
			kind,
			codex_thread_id: row.9,
			fallback_context_pack_id: row.10,
			fallback_runtime_session_id: if kind == ContinuationPlanKind::ContextPackFallback {
				runtime_session.as_ref().map(|session| session.runtime_session_id.clone())
			} else {
				None
			},
			same_thread_evidence,
			replay_permitted: false,
			dispatch_enabled: false,
			planned_at_micros: row.13,
		},
		runtime_session,
		fallback_context_pack,
		uncertain_predecessor_attempt_id,
	})
}

#[allow(clippy::too_many_arguments)]
fn persist_context_pack(
	transaction: &rusqlite::Transaction<'_>,
	context_pack_id: &str,
	conversation_id: &str,
	pack: &ContextPack,
	manifest_json: &str,
	manifest_digest: &str,
	compiled_digest: BlobHash,
	byte_length: i64,
	omitted_source_count: i64,
	created_at_micros: i64,
) -> Result<(), StoreError> {
	let policy = pack.policy();
	transaction
		.execute(
			"INSERT INTO context_packs (
			 context_pack_id, conversation_id, pack_revision, possible_side_effects,
			 policy_max_bytes, policy_recent_item_limit, manifest_json, manifest_sha256,
			 compiled_sha256, byte_length, truncated, omitted_source_count, created_at_micros
			 ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
			params![
				context_pack_id,
				conversation_id,
				side_effects_text(pack.possible_side_effects()),
				i64::try_from(policy.max_bytes())
					.map_err(|_| StoreError::InvalidInput("Context Pack policy is too large"))?,
				i64::try_from(policy.recent_item_limit())
					.map_err(|_| StoreError::InvalidInput("Context Pack policy is too large"))?,
				manifest_json,
				manifest_digest,
				compiled_digest.to_hex(),
				byte_length,
				pack.truncated(),
				omitted_source_count,
				created_at_micros,
			],
		)
		.map_err(sql_error)?;
	Ok(())
}

fn read_context_pack(
	connection: &rusqlite::Connection,
	blob_store: &BlobStore,
	context_pack_id: &str,
	expected_conversation_id: &ConversationId,
) -> Result<ContextPackRecord, StoreError> {
	let row = connection
		.query_row(
			"SELECT conversation_id, pack_revision, possible_side_effects, policy_max_bytes,
			        policy_recent_item_limit, manifest_json, manifest_sha256, compiled_sha256,
			        byte_length, truncated, omitted_source_count
			 FROM context_packs WHERE context_pack_id = ?1",
			params![context_pack_id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, i64>(3)?,
					row.get::<_, i64>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, String>(6)?,
					row.get::<_, String>(7)?,
					row.get::<_, i64>(8)?,
					row.get::<_, bool>(9)?,
					row.get::<_, i64>(10)?,
				))
			},
		)
		.map_err(sql_error)?;
	let conversation_id =
		ConversationId::new(row.0).map_err(|_| incompatible("Context Pack Conversation"))?;
	if &conversation_id != expected_conversation_id || row.1 != 1 {
		return Err(incompatible("Context Pack lineage"));
	}
	let possible_side_effects = parse_side_effects(&row.2)?;
	let max_bytes = usize::try_from(row.3).map_err(|_| incompatible("Context Pack policy"))?;
	let recent_item_limit =
		usize::try_from(row.4).map_err(|_| incompatible("Context Pack policy"))?;
	let policy = ContextPackPolicy::new(max_bytes, recent_item_limit)
		.map_err(|_| incompatible("Context Pack policy"))?;
	let manifest = parse_context_manifest(&row.5)?;
	let compiled_digest =
		BlobHash::parse(&row.7).map_err(|_| incompatible("Context Pack digest"))?;
	let bytes = blob_store.read(compiled_digest)?;
	let pack = ContextPack::from_persisted(
		conversation_id.clone(),
		possible_side_effects,
		policy,
		manifest,
		bytes,
		compiled_digest,
	)
	.map_err(|_| incompatible("Context Pack content"))?;
	let byte_length = u64::try_from(row.8).map_err(|_| incompatible("Context Pack length"))?;
	let omitted_source_count =
		usize::try_from(row.10).map_err(|_| incompatible("Context Pack omission count"))?;
	if pack.manifest_digest().to_hex() != row.6
		|| pack.bytes().len() as u64 != byte_length
		|| pack.truncated() != row.9
		|| pack.omitted_source_count() != omitted_source_count
	{
		return Err(incompatible("Context Pack metadata"));
	}
	Ok(ContextPackRecord {
		context_pack_id: context_pack_id.to_owned(),
		conversation_id,
		pack_revision: row.1,
		compiled_digest,
		byte_length,
		truncated: row.9,
		omitted_source_count,
		pack,
	})
}

fn serialize_context_manifest(pack: &ContextPack) -> Result<String, StoreError> {
	let rows = pack
		.source_manifest()
		.iter()
		.map(|source| PersistedContextSourceManifest {
			kind: source.kind(),
			source_id: source.source_id().to_owned(),
			revision: source.revision(),
			content_digest: source.content_digest().to_hex(),
			original_byte_length: source.original_byte_length(),
			included_byte_length: source.included_byte_length(),
			included_digest: source.included_digest().to_hex(),
			disposition: disposition_text(source.disposition()).to_owned(),
			artifact_id: source.artifact_reference().map(|(id, _)| id.as_str().to_owned()),
			artifact_revision: source.artifact_reference().map(|(_, revision)| revision),
		})
		.collect::<Vec<_>>();
	serde_json::to_string(&rows)
		.map_err(|_| StoreError::InvalidInput("Context Pack manifest is invalid"))
}

fn parse_context_manifest(value: &str) -> Result<Vec<ContextSourceManifest>, StoreError> {
	let rows: Vec<PersistedContextSourceManifest> =
		serde_json::from_str(value).map_err(|_| incompatible("Context Pack manifest"))?;
	rows.into_iter()
		.map(|row| {
			let artifact = match (row.artifact_id, row.artifact_revision) {
				(Some(id), Some(revision)) => Some((
					ArtifactId::new(id).map_err(|_| incompatible("Context Pack Artifact"))?,
					revision,
				)),
				(None, None) => None,
				_ => return Err(incompatible("Context Pack Artifact")),
			};
			ContextSourceManifest::from_persisted(
				row.kind,
				row.source_id,
				row.revision,
				BlobHash::parse(&row.content_digest)
					.map_err(|_| incompatible("Context Pack source digest"))?,
				row.original_byte_length,
				row.included_byte_length,
				BlobHash::parse(&row.included_digest)
					.map_err(|_| incompatible("Context Pack included digest"))?,
				parse_disposition(&row.disposition)?,
				artifact,
			)
			.map_err(|_| incompatible("Context Pack manifest source"))
		})
		.collect()
}

const fn side_effects_text(value: PossibleSideEffects) -> &'static str {
	match value {
		PossibleSideEffects::None => "none",
		PossibleSideEffects::Possible => "possible",
		PossibleSideEffects::Unknown => "unknown",
	}
}

fn parse_side_effects(value: &str) -> Result<PossibleSideEffects, StoreError> {
	match value {
		"none" => Ok(PossibleSideEffects::None),
		"possible" => Ok(PossibleSideEffects::Possible),
		"unknown" => Ok(PossibleSideEffects::Unknown),
		_ => Err(incompatible("Context Pack side-effect state")),
	}
}

const fn disposition_text(value: ContextSourceDisposition) -> &'static str {
	match value {
		ContextSourceDisposition::Complete => "complete",
		ContextSourceDisposition::Truncated => "truncated",
		ContextSourceDisposition::Omitted => "omitted",
	}
}

fn parse_disposition(value: &str) -> Result<ContextSourceDisposition, StoreError> {
	match value {
		"complete" => Ok(ContextSourceDisposition::Complete),
		"truncated" => Ok(ContextSourceDisposition::Truncated),
		"omitted" => Ok(ContextSourceDisposition::Omitted),
		_ => Err(incompatible("Context Pack source disposition")),
	}
}

fn initial_request_sha(request: &PlanInitialThreadContinuation) -> String {
	digest(&[
		&request.operation_id,
		&request.routing_decision_id,
		&request.expected_conversation_revision.to_string(),
		&request.plan_id,
	])
}

fn continuation_request_sha(request: &PlanContinuation) -> String {
	digest(&[
		&request.operation_id,
		&request.routing_decision_id,
		&request.expected_consumer_revision.to_string(),
		&request.plan_id,
		&request.fallback_runtime_session_id,
		&request.fallback_account_snapshot_id,
		&request.fallback_context_pack_id,
	])
}

fn validate_key(key: &str) -> Result<(), StoreError> {
	if key.is_empty() || key.len() > 256 || decodex_core::contains_credential_material(key) {
		return Err(StoreError::InvalidInput("idempotency key is invalid"));
	}
	Ok(())
}

fn incompatible(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
