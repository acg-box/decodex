//! Inert continuation plans over one immutable routing decision.

use decodex_core::{
	AccountId, BlobHash, BlobStore, ContextPack, ContinuationCommandOutcome, ContinuationPlan,
	ContinuationPlanKind, ContinuationRejection, ConversationId, ExecutionConsumer,
	ProviderAttemptId, ProviderEvidenceId, RuntimeSessionId, SameThreadContinuationEvidence,
	TurnId,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
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
				let effect = read_plan_effect(&transaction, &plan_id)?;
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
			let effect = read_plan_effect(&transaction, &request.plan_id)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ContinuationCommandOutcome::Success(effect))
		})
		.await
	}

	/// Plan same-thread continuation from exact terminal provider evidence.
	///
	/// Context-Pack fallback remains deferred in the first local slice; absence of exact positive
	/// thread evidence is a typed refusal, never an implicit new provider intent.
	pub async fn plan_continuation(
		&self,
		_blob_store: &BlobStore,
		idempotency_key: &str,
		request: &PlanContinuation,
		_fallback_pack: &ContextPack,
	) -> Result<ContinuationCommandOutcome<ContinuationPlanEffect>, StoreError> {
		validate_key(idempotency_key)?;
		if request.expected_consumer_revision <= 0 {
			return Err(StoreError::InvalidInput("execution consumer revision must be positive"));
		}
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
				let effect = read_plan_effect(&transaction, &plan_id)?;
				transaction.commit().map_err(sql_error)?;
				return Ok(ContinuationCommandOutcome::Success(effect));
			}
			let authority = transaction
				.query_row(
					"SELECT d.conversation_id, d.turn_id, d.source_runtime_session_id,
				        d.source_runtime_session_revision, d.account_id, s.codex_thread_id,
				        p.attempt_id, p.revision, e.evidence_id
				 FROM routing_decisions AS d
				 JOIN runtime_sessions AS s ON s.runtime_session_id = d.source_runtime_session_id
				 JOIN provider_attempts AS p ON p.runtime_session_id = s.runtime_session_id
				 JOIN provider_attempt_positive_evidence AS e ON e.attempt_id = p.attempt_id
				 WHERE d.routing_decision_id = ?1
				   AND d.authority_shape = 'conversation_continuation'
				   AND d.conversation_revision = ?2 AND d.decision_kind = 'selected'
				   AND s.state = 'active' AND s.revision = d.source_runtime_session_revision
				   AND p.state IN ('succeeded', 'failed_definitive')
				   AND e.provider_thread_id = s.codex_thread_id
				 ORDER BY p.created_at_micros DESC LIMIT 1",
					params![request.routing_decision_id, request.expected_consumer_revision],
					|row| {
						Ok((
							row.get::<_, String>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, String>(2)?,
							row.get::<_, i64>(3)?,
							row.get::<_, String>(4)?,
							row.get::<_, String>(5)?,
							row.get::<_, String>(6)?,
							row.get::<_, i64>(7)?,
							row.get::<_, String>(8)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some(authority) = authority else {
				return Ok(ContinuationCommandOutcome::Rejected(
					ContinuationRejection::SameThreadUnavailable,
				));
			};
			let now = unix_micros().map_err(StoreError::from)?;
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
						authority.0,
						authority.1,
						request.routing_decision_id,
						authority.2,
						authority.3,
						authority.4,
						authority.5,
						authority.6,
						authority.8,
						now,
					],
				)
				.map_err(sql_error)?;
			let effect = read_plan_effect(&transaction, &request.plan_id)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ContinuationCommandOutcome::Success(effect))
		})
		.await
	}
}

fn read_plan_effect(
	connection: &rusqlite::Connection,
	plan_id: &str,
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
		fallback_context_pack: None,
	})
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
