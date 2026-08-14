//! Sole durable writer for provider-attempt preparation, dispatch fencing, and evidence.

use decodex_core::{
	AccountId, ConversationId, ProcessExecutionEpochId, ProcessGenerationId, ProviderAttempt,
	ProviderAttemptConsumer, ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState,
	ProviderAttemptUnknownReason, ProviderDuplicateRisk, ProviderEvidenceId,
	ProviderPositiveEvidence, ProviderRequestId, ProviderRequestKey, ProviderRequestKeys,
	RuntimeSessionId, TurnId,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::{
	RuntimeSessionThreadBindingReadback, SqliteStore, StoreError, account_lifecycle::sql_error,
	unix_micros,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionBindingReceipt {
	idempotency_key: String,
}

impl RuntimeSessionBindingReceipt {
	pub fn from_binding(binding: &RuntimeSessionThreadBindingReadback) -> Self {
		Self { idempotency_key: binding.binding_idempotency_key.clone() }
	}
}

#[derive(Debug, Eq, PartialEq)]
struct ConversationTurnFence {
	conversation_id: ConversationId,
	conversation_revision: i64,
	turn_id: TurnId,
	turn_revision: i64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct FreshPreparedProviderAttempt {
	attempt_id: ProviderAttemptId,
	revision: i64,
	prepared_at_micros: i64,
	conversation_turn_fence: Option<ConversationTurnFence>,
}

impl FreshPreparedProviderAttempt {
	pub fn attempt_id(&self) -> &ProviderAttemptId {
		&self.attempt_id
	}

	pub const fn revision(&self) -> i64 {
		self.revision
	}

	pub const fn prepared_at_micros(&self) -> i64 {
		self.prepared_at_micros
	}
}

#[derive(Debug, Eq, PartialEq)]
pub struct FreshProviderDispatchFence {
	attempt_id: ProviderAttemptId,
	attempt_revision: i64,
	process_generation_id: ProcessGenerationId,
	process_generation_revision: i64,
	authorized_at_micros: i64,
}

impl FreshProviderDispatchFence {
	pub fn attempt_id(&self) -> &ProviderAttemptId {
		&self.attempt_id
	}

	pub const fn attempt_revision(&self) -> i64 {
		self.attempt_revision
	}

	pub fn process_generation_id(&self) -> &ProcessGenerationId {
		&self.process_generation_id
	}

	pub const fn process_generation_revision(&self) -> i64 {
		self.process_generation_revision
	}

	pub const fn authorized_at_micros(&self) -> i64 {
		self.authorized_at_micros
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttemptMutation {
	pub revision: i64,
	pub state: ProviderAttemptState,
	pub recorded_at_micros: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptRejection {
	IdentityConflict,
	AuthorityUnavailable,
	GenerationUnavailable,
	ConsumerUnavailable,
	InvalidInput,
	AttemptMissing,
	StaleAttempt,
	EvidenceConflict,
	InvalidEvidence,
	EvidenceMismatch,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PrepareProviderAttemptOutcome {
	Fresh(FreshPreparedProviderAttempt),
	Replayed(ProviderAttemptMutation),
	Rejected { rejection: ProviderAttemptRejection, actual: ProviderAttemptMutation },
}

#[derive(Debug, Eq, PartialEq)]
pub enum AuthorizeProviderDispatchOutcome {
	Fresh(FreshProviderDispatchFence),
	Replayed(ProviderAttemptMutation),
	Rejected { rejection: ProviderAttemptRejection, actual: ProviderAttemptMutation },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderAttemptMutationOutcome {
	Applied(ProviderAttemptMutation),
	Replayed(ProviderAttemptMutation),
	Rejected { rejection: ProviderAttemptRejection, actual: ProviderAttemptMutation },
}

impl SqliteStore {
	#[allow(clippy::too_many_arguments)]
	#[allow(clippy::too_many_lines)] // Keep one atomic dispatch-intent transaction together.
	pub async fn prepare_provider_attempt(
		&self,
		preparation: &ProviderAttemptPreparation,
		process_generation_id: &ProcessGenerationId,
		process_generation_revision: i64,
		process_execution_epoch_id: &ProcessExecutionEpochId,
		binding_receipt: Option<&RuntimeSessionBindingReceipt>,
		expected_conversation_turn_revisions: (Option<i64>, Option<i64>),
	) -> Result<PrepareProviderAttemptOutcome, StoreError> {
		if process_generation_revision <= 0 {
			return Err(StoreError::InvalidInput(
				"ProviderAttempt generation revision must be positive",
			));
		}
		let (conversation_id, turn_id) = match &preparation.consumer {
			ProviderAttemptConsumer::ConversationTurn { conversation_id, turn_id } =>
				(conversation_id.clone(), turn_id.clone()),
			ProviderAttemptConsumer::ManagedRunExecution { .. } => {
				return Err(StoreError::InvalidInput("ManagedRun ProviderAttempt is deferred"));
			},
		};
		let (conversation_revision, turn_revision) = expected_conversation_turn_revisions;
		let conversation_revision = conversation_revision.ok_or(StoreError::InvalidInput(
			"Conversation ProviderAttempt requires a Conversation revision",
		))?;
		let turn_revision = turn_revision.ok_or(StoreError::InvalidInput(
			"Conversation ProviderAttempt requires a Turn revision",
		))?;
		if conversation_revision <= 0 || turn_revision != 1 {
			return Err(StoreError::InvalidInput(
				"Conversation ProviderAttempt revision is invalid",
			));
		}
		let preparation = preparation.clone();
		let process_generation_id = process_generation_id.clone();
		let process_execution_epoch_id = process_execution_epoch_id.clone();
		let binding_key = binding_receipt.map(|value| value.idempotency_key.clone());
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(existing) = read_attempt(&transaction, preparation.attempt_id.as_str())? {
				let same = existing.consumer == preparation.consumer
					&& existing.continuation_plan_id == preparation.continuation_plan_id
					&& existing.process_generation_id == process_generation_id
					&& existing.request_id == preparation.request_id
					&& existing.request_digest == preparation.request_digest
					&& existing.provider_keys == preparation.provider_keys
					&& existing.duplicate_risk == preparation.duplicate_risk;
				let actual = mutation(&existing);
				transaction.commit().map_err(sql_error)?;
				return if same {
					Ok(PrepareProviderAttemptOutcome::Replayed(actual))
				} else {
					Ok(PrepareProviderAttemptOutcome::Rejected {
						rejection: ProviderAttemptRejection::IdentityConflict,
						actual,
					})
				};
			}
			let authority = transaction
				.query_row(
					"SELECT p.routing_decision_id, p.source_runtime_session_id, s.revision,
				        p.selected_account_id
				 FROM continuation_plans AS p
				 JOIN routing_decisions AS d ON d.routing_decision_id = p.routing_decision_id
				 JOIN runtime_sessions AS s ON s.runtime_session_id = p.source_runtime_session_id
				 JOIN conversations AS c ON c.conversation_id = p.conversation_id
				 JOIN turns AS t ON t.turn_id = p.turn_id
				 JOIN process_generations AS g ON g.generation_id = ?1
				 WHERE p.continuation_plan_id = ?2 AND p.conversation_id = ?3 AND p.turn_id = ?4
				   AND c.revision = ?5 AND c.state = 'active'
				   AND t.revision = ?6 AND t.status = 'active'
				   AND g.revision = ?7 AND g.state = 'ready'
				   AND g.execution_epoch_id = ?8 AND g.account_id = p.selected_account_id
				   AND g.runtime_session_id = p.source_runtime_session_id
				   AND (?9 IS NULL OR s.thread_start_binding_key = ?9)",
					params![
						process_generation_id.as_str(),
						preparation.continuation_plan_id,
						conversation_id.as_str(),
						turn_id.as_str(),
						conversation_revision,
						turn_revision,
						process_generation_revision,
						process_execution_epoch_id.as_str(),
						binding_key,
					],
					|row| {
						Ok((
							row.get::<_, String>(0)?,
							row.get::<_, String>(1)?,
							row.get::<_, i64>(2)?,
							row.get::<_, String>(3)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some((
				routing_decision_id,
				runtime_session_id,
				runtime_session_revision,
				account_id,
			)) = authority
			else {
				return Ok(PrepareProviderAttemptOutcome::Rejected {
					rejection: ProviderAttemptRejection::AuthorityUnavailable,
					actual: empty_mutation(),
				});
			};
			let (idempotency, correlation) = provider_keys(&preparation.provider_keys);
			let (predecessor, acknowledgement) = duplicate_risk(&preparation.duplicate_risk);
			let now = unix_micros().map_err(StoreError::from)?;
			transaction
				.execute(
					"INSERT INTO provider_attempts (
				   attempt_id, conversation_id, turn_id, continuation_plan_id,
				   routing_decision_id, runtime_session_id, runtime_session_revision,
				   account_id, process_generation_id, process_generation_revision,
				   execution_epoch_id, request_id, request_sha256, provider_idempotency_key,
				   provider_correlation_key, predecessor_attempt_id, duplicate_risk_ack_sha256,
				   state, revision, created_at_micros, updated_at_micros
				 ) VALUES (
				   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
				   ?14, ?15, ?16, ?17, 'prepared', 1, ?18, ?18
				 )",
					params![
						preparation.attempt_id.as_str(),
						conversation_id.as_str(),
						turn_id.as_str(),
						preparation.continuation_plan_id,
						routing_decision_id,
						runtime_session_id,
						runtime_session_revision,
						account_id,
						process_generation_id.as_str(),
						process_generation_revision,
						process_execution_epoch_id.as_str(),
						preparation.request_id.as_str(),
						preparation.request_digest,
						idempotency,
						correlation,
						predecessor,
						acknowledgement,
						now,
					],
				)
				.map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(PrepareProviderAttemptOutcome::Fresh(FreshPreparedProviderAttempt {
				attempt_id: preparation.attempt_id,
				revision: 1,
				prepared_at_micros: now,
				conversation_turn_fence: Some(ConversationTurnFence {
					conversation_id,
					conversation_revision,
					turn_id,
					turn_revision,
				}),
			}))
		})
		.await
	}

	pub async fn authorize_provider_attempt_dispatch(
		&self,
		prepared: FreshPreparedProviderAttempt,
		process_generation_id: &ProcessGenerationId,
		process_generation_revision: i64,
	) -> Result<AuthorizeProviderDispatchOutcome, StoreError> {
		let process_generation_id = process_generation_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let Some(current) = read_attempt(&transaction, prepared.attempt_id.as_str())? else {
				return Ok(AuthorizeProviderDispatchOutcome::Rejected {
					rejection: ProviderAttemptRejection::AttemptMissing,
					actual: empty_mutation(),
				});
			};
			if current.state == ProviderAttemptState::DispatchAuthorized
				&& current.revision == prepared.revision.saturating_add(1)
			{
				let actual = mutation(&current);
				transaction.commit().map_err(sql_error)?;
				return Ok(AuthorizeProviderDispatchOutcome::Replayed(actual));
			}
			let process_ready: bool = transaction
				.query_row(
					"SELECT EXISTS (SELECT 1 FROM process_generations WHERE generation_id = ?1
				 AND revision = ?2 AND state = 'ready')",
					params![process_generation_id.as_str(), process_generation_revision],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			let turn_ready = prepared.conversation_turn_fence.as_ref().is_some_and(|fence| {
				transaction.query_row(
					"SELECT EXISTS (SELECT 1 FROM conversations AS c JOIN turns AS t USING (conversation_id)
					 WHERE c.conversation_id = ?1 AND c.revision = ?2 AND c.state = 'active'
					 AND t.turn_id = ?3 AND t.revision = ?4 AND t.status = 'active')",
					params![fence.conversation_id.as_str(), fence.conversation_revision,
						fence.turn_id.as_str(), fence.turn_revision], |row| row.get::<_, bool>(0),
				).unwrap_or(false)
			});
			if current.revision != prepared.revision
				|| current.state != ProviderAttemptState::Prepared
				|| current.process_generation_id != process_generation_id
				|| current.process_generation_revision != process_generation_revision
				|| !process_ready
				|| !turn_ready
			{
				return Ok(AuthorizeProviderDispatchOutcome::Rejected {
					rejection: ProviderAttemptRejection::StaleAttempt,
					actual: mutation(&current),
				});
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let revision = prepared.revision + 1;
			transaction
				.execute(
					"UPDATE provider_attempts SET state = 'dispatch_authorized', revision = ?1,
				 updated_at_micros = ?2 WHERE attempt_id = ?3",
					params![revision, now, prepared.attempt_id.as_str()],
				)
				.map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(AuthorizeProviderDispatchOutcome::Fresh(FreshProviderDispatchFence {
				attempt_id: prepared.attempt_id,
				attempt_revision: revision,
				process_generation_id,
				process_generation_revision,
				authorized_at_micros: now,
			}))
		})
		.await
	}

	pub async fn cancel_provider_attempt(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		self.transition_attempt(
			attempt_id,
			expected_revision,
			ProviderAttemptState::Prepared,
			ProviderAttemptState::Canceled,
			None,
		)
		.await
	}

	pub async fn mark_provider_attempt_unknown(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
		reason: ProviderAttemptUnknownReason,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		if reason == ProviderAttemptUnknownReason::RestoreProjection {
			return Err(StoreError::InvalidInput("restore projection is not a live transition"));
		}
		self.transition_attempt(
			attempt_id,
			expected_revision,
			ProviderAttemptState::DispatchAuthorized,
			ProviderAttemptState::Unknown,
			Some(reason),
		)
		.await
	}

	pub async fn record_provider_attempt_positive_evidence(
		&self,
		expected_revision: i64,
		evidence: &ProviderPositiveEvidence,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		let evidence = evidence.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let Some(current) = read_attempt(&transaction, evidence.attempt_id.as_str())? else {
				return Ok(ProviderAttemptMutationOutcome::Rejected {
					rejection: ProviderAttemptRejection::AttemptMissing,
					actual: empty_mutation(),
				});
			};
			if current.state.is_terminal() {
				return if current.terminal_evidence_id.as_ref() == Some(&evidence.evidence_id) {
					Ok(ProviderAttemptMutationOutcome::Replayed(mutation(&current)))
				} else {
					Ok(rejected(ProviderAttemptRejection::EvidenceConflict, &current))
				};
			}
			if current.revision != expected_revision
				|| !matches!(
					current.state,
					ProviderAttemptState::DispatchAuthorized | ProviderAttemptState::Unknown
				) || current.request_id != evidence.request_id
				|| !current.provider_keys.contains(&evidence.provider_key)
			{
				return Ok(rejected(ProviderAttemptRejection::EvidenceMismatch, &current));
			}
			let now = unix_micros().map_err(StoreError::from)?;
			transaction
				.execute(
					"INSERT INTO provider_attempt_positive_evidence (
				 evidence_id, attempt_id, request_id, source, outcome, provider_key,
				 provider_receipt_id, provider_thread_id, provider_turn_id, witness_sha256,
				 observed_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
					params![
						evidence.evidence_id.as_str(),
						evidence.attempt_id.as_str(),
						evidence.request_id.as_str(),
						evidence.source.as_sql(),
						evidence.outcome.as_sql(),
						evidence.provider_key.as_str(),
						evidence.provider_receipt_id,
						evidence.provider_thread_id,
						evidence.provider_turn_id,
						evidence.witness_digest,
						now
					],
				)
				.map_err(sql_error)?;
			let revision = expected_revision + 1;
			transaction
				.execute(
					"UPDATE provider_attempts SET state = ?1, unknown_reason = NULL,
				 terminal_evidence_id = ?2, revision = ?3, updated_at_micros = ?4
				 WHERE attempt_id = ?5",
					params![
						evidence.outcome.as_sql(),
						evidence.evidence_id.as_str(),
						revision,
						now,
						evidence.attempt_id.as_str()
					],
				)
				.map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ProviderAttemptMutationOutcome::Applied(ProviderAttemptMutation {
				revision,
				state: evidence.outcome.state(),
				recorded_at_micros: now,
			}))
		})
		.await
	}

	pub async fn project_provider_attempts_after_supervisor_loss(&self) -> Result<u64, StoreError> {
		self.run(move |connection| {
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = connection.execute(
				"UPDATE provider_attempts SET state = 'unknown', unknown_reason = 'restore_projection',
				 revision = revision + 1, updated_at_micros = ?1
				 WHERE state IN ('prepared', 'dispatch_authorized')",
				params![now],
			).map_err(sql_error)?;
			u64::try_from(changed).map_err(|_| incompatible("attempt projection count"))
		})
		.await
	}

	pub async fn read_provider_attempt_page(
		&self,
		account_id: Option<&AccountId>,
		state: Option<ProviderAttemptState>,
		after_attempt_id: Option<&ProviderAttemptId>,
		limit: u16,
	) -> Result<Vec<ProviderAttempt>, StoreError> {
		if !(1..=256).contains(&limit) {
			return Err(StoreError::InvalidInput(
				"ProviderAttempt read limit must be between 1 and 256",
			));
		}
		let account = account_id.map(|value| value.as_str().to_owned());
		let state = state.map(|value| value.as_sql().to_owned());
		let after = after_attempt_id.map(|value| value.as_str().to_owned());
		self.run(move |connection| {
			let mut statement = connection
				.prepare(
					"SELECT attempt_id FROM provider_attempts WHERE (?1 IS NULL OR account_id = ?1)
				 AND (?2 IS NULL OR state = ?2) AND (?3 IS NULL OR attempt_id > ?3)
				 ORDER BY attempt_id LIMIT ?4",
				)
				.map_err(sql_error)?;
			let ids = statement
				.query_map(params![account, state, after, i64::from(limit)], |row| {
					row.get::<_, String>(0)
				})
				.map_err(sql_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(sql_error)?;
			ids.into_iter()
				.map(|id| read_attempt(connection, &id)?.ok_or_else(|| incompatible("attempt")))
				.collect()
		})
		.await
	}

	pub async fn read_provider_attempt(
		&self,
		attempt_id: &ProviderAttemptId,
	) -> Result<Option<ProviderAttempt>, StoreError> {
		let attempt_id = attempt_id.clone();
		self.run(move |connection| read_attempt(connection, attempt_id.as_str())).await
	}

	async fn transition_attempt(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
		expected_state: ProviderAttemptState,
		target_state: ProviderAttemptState,
		reason: Option<ProviderAttemptUnknownReason>,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProviderAttempt revision must be positive"));
		}
		let attempt_id = attempt_id.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let Some(current) = read_attempt(&transaction, attempt_id.as_str())? else {
				return Ok(ProviderAttemptMutationOutcome::Rejected {
					rejection: ProviderAttemptRejection::AttemptMissing,
					actual: empty_mutation(),
				});
			};
			if current.state == target_state
				&& current.revision == expected_revision.saturating_add(1)
				&& current.unknown_reason == reason
			{
				let result = ProviderAttemptMutationOutcome::Replayed(mutation(&current));
				transaction.commit().map_err(sql_error)?;
				return Ok(result);
			}
			if current.revision != expected_revision || current.state != expected_state {
				return Ok(rejected(ProviderAttemptRejection::StaleAttempt, &current));
			}
			let revision = expected_revision + 1;
			let now = unix_micros().map_err(StoreError::from)?;
			transaction
				.execute(
					"UPDATE provider_attempts SET state = ?1, unknown_reason = ?2,
				 revision = ?3, updated_at_micros = ?4 WHERE attempt_id = ?5",
					params![
						target_state.as_sql(),
						reason.map(ProviderAttemptUnknownReason::as_sql),
						revision,
						now,
						attempt_id.as_str()
					],
				)
				.map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ProviderAttemptMutationOutcome::Applied(ProviderAttemptMutation {
				revision,
				state: target_state,
				recorded_at_micros: now,
			}))
		})
		.await
	}
}

fn read_attempt(
	connection: &rusqlite::Connection,
	id: &str,
) -> Result<Option<ProviderAttempt>, StoreError> {
	connection
		.query_row(
			"SELECT attempt_id, conversation_id, turn_id, continuation_plan_id,
		        routing_decision_id, runtime_session_id, runtime_session_revision,
		        account_id, process_generation_id, process_generation_revision,
		        execution_epoch_id, request_id, request_sha256, provider_idempotency_key,
		        provider_correlation_key, predecessor_attempt_id, duplicate_risk_ack_sha256,
		        state, unknown_reason, terminal_evidence_id, revision,
		        created_at_micros, updated_at_micros
		 FROM provider_attempts WHERE attempt_id = ?1",
			params![id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, i64>(6)?,
					row.get::<_, String>(7)?,
					row.get::<_, String>(8)?,
					row.get::<_, i64>(9)?,
					row.get::<_, String>(10)?,
					row.get::<_, String>(11)?,
					row.get::<_, String>(12)?,
					row.get::<_, Option<String>>(13)?,
					row.get::<_, Option<String>>(14)?,
					row.get::<_, Option<String>>(15)?,
					row.get::<_, Option<String>>(16)?,
					row.get::<_, String>(17)?,
					row.get::<_, Option<String>>(18)?,
					row.get::<_, Option<String>>(19)?,
					row.get::<_, i64>(20)?,
					row.get::<_, i64>(21)?,
					row.get::<_, i64>(22)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?
		.map(parse_attempt_row)
		.transpose()
}

#[allow(clippy::type_complexity)]
type AttemptRow = (
	String,
	String,
	String,
	String,
	String,
	String,
	i64,
	String,
	String,
	i64,
	String,
	String,
	String,
	Option<String>,
	Option<String>,
	Option<String>,
	Option<String>,
	String,
	Option<String>,
	Option<String>,
	i64,
	i64,
	i64,
);

fn parse_attempt_row(row: AttemptRow) -> Result<ProviderAttempt, StoreError> {
	let idempotency = row
		.13
		.map(ProviderRequestKey::new)
		.transpose()
		.map_err(|_| incompatible("provider idempotency key"))?;
	let correlation = row
		.14
		.map(ProviderRequestKey::new)
		.transpose()
		.map_err(|_| incompatible("provider correlation key"))?;
	let provider_keys = ProviderRequestKeys::new(idempotency, correlation)
		.map_err(|_| incompatible("provider keys"))?;
	let duplicate_risk = match (row.15, row.16) {
		(None, None) => ProviderDuplicateRisk::OriginalIntent,
		(Some(predecessor), Some(acknowledgement_digest)) =>
			ProviderDuplicateRisk::AcknowledgedSuccessor {
				predecessor_attempt_id: ProviderAttemptId::new(predecessor)
					.map_err(|_| incompatible("predecessor attempt"))?,
				acknowledgement_digest,
			},
		_ => return Err(incompatible("duplicate-risk shape")),
	};
	Ok(ProviderAttempt {
		attempt_id: ProviderAttemptId::new(row.0).map_err(|_| incompatible("attempt id"))?,
		consumer: ProviderAttemptConsumer::ConversationTurn {
			conversation_id: ConversationId::new(row.1)
				.map_err(|_| incompatible("Conversation"))?,
			turn_id: TurnId::new(row.2).map_err(|_| incompatible("Turn"))?,
		},
		continuation_plan_id: row.3,
		routing_decision_id: row.4,
		runtime_session_id: RuntimeSessionId::new(row.5)
			.map_err(|_| incompatible("RuntimeSession"))?,
		runtime_session_revision: row.6,
		account_id: AccountId::new(row.7).map_err(|_| incompatible("account"))?,
		process_generation_id: ProcessGenerationId::new(row.8)
			.map_err(|_| incompatible("ProcessGeneration"))?,
		process_generation_revision: row.9,
		process_execution_epoch_id: ProcessExecutionEpochId::new(row.10)
			.map_err(|_| incompatible("execution epoch"))?,
		request_id: ProviderRequestId::new(row.11).map_err(|_| incompatible("request id"))?,
		request_digest: row.12,
		provider_keys,
		duplicate_risk,
		state: parse_state(&row.17)?,
		unknown_reason: row.18.as_deref().map(parse_unknown).transpose()?,
		terminal_evidence_id: row
			.19
			.map(ProviderEvidenceId::new)
			.transpose()
			.map_err(|_| incompatible("terminal evidence"))?,
		revision: row.20,
		created_at_micros: row.21,
		updated_at_micros: row.22,
	})
}

fn provider_keys(keys: &ProviderRequestKeys) -> (Option<&str>, Option<&str>) {
	(
		keys.idempotency().map(ProviderRequestKey::as_str),
		keys.correlation().map(ProviderRequestKey::as_str),
	)
}

fn duplicate_risk(risk: &ProviderDuplicateRisk) -> (Option<&str>, Option<&str>) {
	match risk {
		ProviderDuplicateRisk::OriginalIntent => (None, None),
		ProviderDuplicateRisk::AcknowledgedSuccessor {
			predecessor_attempt_id,
			acknowledgement_digest,
		} => (Some(predecessor_attempt_id.as_str()), Some(acknowledgement_digest)),
	}
}

fn parse_state(value: &str) -> Result<ProviderAttemptState, StoreError> {
	match value {
		"prepared" => Ok(ProviderAttemptState::Prepared),
		"canceled" => Ok(ProviderAttemptState::Canceled),
		"dispatch_authorized" => Ok(ProviderAttemptState::DispatchAuthorized),
		"succeeded" => Ok(ProviderAttemptState::Succeeded),
		"failed_definitive" => Ok(ProviderAttemptState::FailedDefinitive),
		"not_submitted" => Ok(ProviderAttemptState::NotSubmitted),
		"unknown" => Ok(ProviderAttemptState::Unknown),
		_ => Err(incompatible("attempt state")),
	}
}

fn parse_unknown(value: &str) -> Result<ProviderAttemptUnknownReason, StoreError> {
	match value {
		"supervision_lost" => Ok(ProviderAttemptUnknownReason::SupervisionLost),
		"dispatch_outcome_unavailable" =>
			Ok(ProviderAttemptUnknownReason::DispatchOutcomeUnavailable),
		"restore_projection" => Ok(ProviderAttemptUnknownReason::RestoreProjection),
		_ => Err(incompatible("attempt unknown reason")),
	}
}

fn mutation(attempt: &ProviderAttempt) -> ProviderAttemptMutation {
	ProviderAttemptMutation {
		revision: attempt.revision,
		state: attempt.state,
		recorded_at_micros: attempt.updated_at_micros,
	}
}

fn rejected(
	rejection: ProviderAttemptRejection,
	attempt: &ProviderAttempt,
) -> ProviderAttemptMutationOutcome {
	ProviderAttemptMutationOutcome::Rejected { rejection, actual: mutation(attempt) }
}

fn empty_mutation() -> ProviderAttemptMutation {
	ProviderAttemptMutation {
		revision: 0,
		state: ProviderAttemptState::Prepared,
		recorded_at_micros: 0,
	}
}

fn incompatible(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
