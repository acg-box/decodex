//! Least-privilege PostgreSQL capability for the sole ProviderAttempt writer.
//!
//! Runtime can execute these closed functions but cannot read or write the underlying relations.
//! Only a freshly committed authorization can mint a dispatch fence. A durable replay cannot.

use decodex_core::{
	AccountId, ConversationId, ManagedExecutionId, ManagedRunId, ProcessExecutionEpochId,
	ProcessGenerationId, ProviderAttempt, ProviderAttemptConsumer, ProviderAttemptId,
	ProviderAttemptPreparation, ProviderAttemptState, ProviderAttemptUnknownReason,
	ProviderDuplicateRisk, ProviderEvidenceId, ProviderPositiveEvidence, ProviderRequestId,
	ProviderRequestKey, ProviderRequestKeys, RuntimeSessionId, TurnId,
};

use crate::{
	PostgresStore, RuntimeSessionThreadBindingReadback, StoreError,
	exact_commands::{EXACT_COMMAND_PROTOCOL, validate_exact_key},
};

const PREPARE_PROVIDER_ATTEMPT_SQL: &str = "SELECT result_code,revision,state::text,created_at_micros,updated_at_micros \
	 FROM decodex.prepare_provider_attempt_exact(\
	 $1::text::uuid,$2::text::decodex.provider_attempt_consumer_kind,\
	 $3::text::uuid,$4::text::uuid,$5::text::uuid,$6,$7::text::uuid,\
	 $8::text::uuid,$9::text::uuid,$10,$11::text::uuid,\
	 $12::text::uuid,$13,$14,$15,$16::text::uuid,$17,$18,$19,$20,$21)";
const AUTHORIZE_PROVIDER_ATTEMPT_DISPATCH_SQL: &str = "SELECT \
	 result_code,revision,state::text,updated_at_micros \
	 FROM decodex.authorize_provider_attempt_dispatch_exact(\
	 $1::text::uuid,$2,$3::text::uuid,$4,$5::text::uuid,$6,$7::text::uuid,$8)";

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_provider_attempt_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	client.prepare(PREPARE_PROVIDER_ATTEMPT_SQL).await?;
	client.prepare(AUTHORIZE_PROVIDER_ATTEMPT_DISPATCH_SQL).await?;
	Ok(2)
}

#[derive(Debug, Eq, PartialEq)]
struct ConversationTurnFence {
	conversation_id: ConversationId,
	conversation_revision: i64,
	turn_id: TurnId,
	turn_revision: i64,
}

/// Exact completed RuntimeSession bind receipt admitted by initial-thread preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionBindingReceipt {
	idempotency_key: String,
}

impl RuntimeSessionBindingReceipt {
	/// Derive the receipt identity from a strict RuntimeSession binding readback.
	pub fn from_binding(binding: &RuntimeSessionThreadBindingReadback) -> Self {
		Self { idempotency_key: binding.binding_idempotency_key.clone() }
	}

	const fn protocol_version(&self) -> &'static str {
		EXACT_COMMAND_PROTOCOL
	}

	fn idempotency_key(&self) -> &str {
		&self.idempotency_key
	}
}

/// Newly committed prepared authority. Durable replay cannot construct this value.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshPreparedProviderAttempt {
	attempt_id: ProviderAttemptId,
	revision: i64,
	prepared_at_micros: i64,
	conversation_turn_fence: Option<ConversationTurnFence>,
}
impl FreshPreparedProviderAttempt {
	/// Return the exact newly prepared attempt.
	pub fn attempt_id(&self) -> &ProviderAttemptId {
		&self.attempt_id
	}

	/// Return the committed prepared revision.
	pub const fn revision(&self) -> i64 {
		self.revision
	}

	/// Return the PostgreSQL-authored preparation time.
	pub const fn prepared_at_micros(&self) -> i64 {
		self.prepared_at_micros
	}
}

/// One-time dispatch authority minted only by a fresh committed state transition.
///
/// This value carries no provider transport, credentials, request bytes, or retry operation.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshProviderDispatchFence {
	attempt_id: ProviderAttemptId,
	attempt_revision: i64,
	process_generation_id: ProcessGenerationId,
	process_generation_revision: i64,
	authorized_at_micros: i64,
}
impl FreshProviderDispatchFence {
	/// Return the exact attempt authorized once.
	pub fn attempt_id(&self) -> &ProviderAttemptId {
		&self.attempt_id
	}

	/// Return the committed dispatch-authorized revision.
	pub const fn attempt_revision(&self) -> i64 {
		self.attempt_revision
	}

	/// Return the exact still-ready generation fenced by the transaction.
	pub fn process_generation_id(&self) -> &ProcessGenerationId {
		&self.process_generation_id
	}

	/// Return the exact ready generation revision.
	pub const fn process_generation_revision(&self) -> i64 {
		self.process_generation_revision
	}

	/// Return the PostgreSQL-authored authorization time.
	pub const fn authorized_at_micros(&self) -> i64 {
		self.authorized_at_micros
	}
}

/// One exact durable ProviderAttempt mutation projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttemptMutation {
	/// Current durable revision.
	pub revision: i64,
	/// Current durable state.
	pub state: ProviderAttemptState,
	/// PostgreSQL-authored transition or observation time.
	pub recorded_at_micros: i64,
}

/// Stable rejection from the ProviderAttempt authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderAttemptRejection {
	/// Attempt identity, request identity, plan, consumer, or provider key was already assigned.
	IdentityConflict,
	/// Routing Decision, Continuation Plan, RuntimeSession, or restore authority was unavailable.
	AuthorityUnavailable,
	/// The bound ProcessGeneration was absent, stale, or no longer ready.
	GenerationUnavailable,
	/// The exact Conversation turn or ManagedRun execution authority was unavailable.
	ConsumerUnavailable,
	/// The caller supplied an invalid closed input shape.
	InvalidInput,
	/// The exact attempt does not exist.
	AttemptMissing,
	/// State, revision, generation, or result authority was stale.
	StaleAttempt,
	/// Positive evidence identity was already assigned to different facts.
	EvidenceConflict,
	/// Positive evidence did not match a supported closed shape.
	InvalidEvidence,
	/// Positive evidence did not match the original request, provider key, or thread.
	EvidenceMismatch,
}

/// Result of one atomic preparation transaction.
#[derive(Debug, Eq, PartialEq)]
pub enum PrepareProviderAttemptOutcome {
	/// Preparation committed for the first time.
	Fresh(FreshPreparedProviderAttempt),
	/// The exact preparation was already durable. This result grants no dispatch authority.
	Replayed(ProviderAttemptMutation),
	/// PostgreSQL rejected the preparation.
	Rejected {
		/// Stable rejection.
		rejection: ProviderAttemptRejection,
		/// Existing projection when one exists.
		actual: ProviderAttemptMutation,
	},
}

/// Result of dispatch authorization. Only `Fresh` can reach a future provider gateway.
#[derive(Debug, Eq, PartialEq)]
pub enum AuthorizeProviderDispatchOutcome {
	/// Exactly one dispatch authorization committed now.
	Fresh(FreshProviderDispatchFence),
	/// Dispatch authorization was already durable. No new dispatch fence exists.
	Replayed(ProviderAttemptMutation),
	/// PostgreSQL rejected dispatch authorization.
	Rejected {
		/// Stable rejection.
		rejection: ProviderAttemptRejection,
		/// Current durable projection.
		actual: ProviderAttemptMutation,
	},
}

/// Result of a non-dispatch attempt mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderAttemptMutationOutcome {
	/// The requested transition or positive evidence committed.
	Applied(ProviderAttemptMutation),
	/// The same exact transition or evidence was already durable.
	Replayed(ProviderAttemptMutation),
	/// PostgreSQL rejected the mutation.
	Rejected {
		/// Stable rejection.
		rejection: ProviderAttemptRejection,
		/// Current durable projection.
		actual: ProviderAttemptMutation,
	},
}

impl PostgresStore {
	/// Atomically bind one prepared attempt to one consumer, Routing Decision/Continuation Plan,
	/// and live generation.
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
				"ProviderAttempt ProcessGeneration revision must be positive",
			));
		}
		let (expected_conversation_revision, expected_turn_revision) =
			expected_conversation_turn_revisions;
		let (conversation_id, turn_id, managed_run_id, managed_run_revision, managed_execution_id) =
			consumer_parameters(&preparation.consumer);
		let conversation_turn_fence = match &preparation.consumer {
			ProviderAttemptConsumer::ConversationTurn { conversation_id, turn_id } => {
				let conversation_revision =
					expected_conversation_revision.ok_or(StoreError::InvalidInput(
						"Conversation ProviderAttempt requires an exact Conversation revision",
					))?;
				let turn_revision = expected_turn_revision.ok_or(StoreError::InvalidInput(
					"Conversation ProviderAttempt requires an exact Turn revision",
				))?;
				if conversation_revision <= 0 || turn_revision != 1 {
					return Err(StoreError::InvalidInput(
						"Conversation ProviderAttempt requires active Turn revision 1",
					));
				}
				Some(ConversationTurnFence {
					conversation_id: conversation_id.clone(),
					conversation_revision,
					turn_id: turn_id.clone(),
					turn_revision,
				})
			},
			ProviderAttemptConsumer::ManagedRunExecution { .. } => {
				if expected_conversation_revision.is_some() || expected_turn_revision.is_some() {
					return Err(StoreError::InvalidInput(
						"ManagedRun ProviderAttempt cannot carry Conversation Turn authority",
					));
				}
				None
			},
		};
		let provider_idempotency_key =
			preparation.provider_keys.idempotency().map(ProviderRequestKey::as_str);
		let provider_correlation_key =
			preparation.provider_keys.correlation().map(ProviderRequestKey::as_str);
		let (predecessor_attempt_id, duplicate_risk_ack_digest) =
			duplicate_risk_parameters(&preparation.duplicate_risk);
		if let Some(receipt) = binding_receipt {
			validate_exact_key(receipt.idempotency_key())?;
		}
		let binding_protocol = binding_receipt.map(RuntimeSessionBindingReceipt::protocol_version);
		let binding_idempotency_key =
			binding_receipt.map(RuntimeSessionBindingReceipt::idempotency_key);
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				PREPARE_PROVIDER_ATTEMPT_SQL,
				&[
					&preparation.attempt_id.as_str(),
					&preparation.consumer.as_sql(),
					&conversation_id,
					&turn_id,
					&managed_run_id,
					&managed_run_revision,
					&managed_execution_id,
					&preparation.continuation_plan_id,
					&process_generation_id.as_str(),
					&process_generation_revision,
					&process_execution_epoch_id.as_str(),
					&preparation.request_id.as_str(),
					&preparation.request_digest,
					&provider_idempotency_key,
					&provider_correlation_key,
					&predecessor_attempt_id,
					&duplicate_risk_ack_digest,
					&binding_protocol,
					&binding_idempotency_key,
					&expected_conversation_revision,
					&expected_turn_revision,
				],
			)
			.await?;
		let result_code: &str = row.get(0);
		let mutation = parse_mutation(&row, 1, 2, 4)?;
		match result_code {
			"prepared" => Ok(PrepareProviderAttemptOutcome::Fresh(FreshPreparedProviderAttempt {
				attempt_id: preparation.attempt_id.clone(),
				revision: mutation.revision,
				prepared_at_micros: row.get(3),
				conversation_turn_fence,
			})),
			"replayed" => Ok(PrepareProviderAttemptOutcome::Replayed(mutation)),
			code => Ok(PrepareProviderAttemptOutcome::Rejected {
				rejection: parse_rejection(code)?,
				actual: mutation,
			}),
		}
	}

	/// Commit exactly one dispatch authorization while the bound generation remains ready.
	pub async fn authorize_provider_attempt_dispatch(
		&self,
		prepared: FreshPreparedProviderAttempt,
		process_generation_id: &ProcessGenerationId,
		process_generation_revision: i64,
	) -> Result<AuthorizeProviderDispatchOutcome, StoreError> {
		if process_generation_revision <= 0 {
			return Err(StoreError::InvalidInput(
				"ProviderAttempt ProcessGeneration revision must be positive",
			));
		}
		let (conversation_id, conversation_revision, turn_id, turn_revision) = prepared
			.conversation_turn_fence
			.as_ref()
			.map(|fence| {
				(
					Some(fence.conversation_id.as_str()),
					Some(fence.conversation_revision),
					Some(fence.turn_id.as_str()),
					Some(fence.turn_revision),
				)
			})
			.unwrap_or((None, None, None, None));
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				AUTHORIZE_PROVIDER_ATTEMPT_DISPATCH_SQL,
				&[
					&prepared.attempt_id.as_str(),
					&prepared.revision,
					&process_generation_id.as_str(),
					&process_generation_revision,
					&conversation_id,
					&conversation_revision,
					&turn_id,
					&turn_revision,
				],
			)
			.await?;
		let result_code: &str = row.get(0);
		let mutation = parse_mutation(&row, 1, 2, 3)?;
		match result_code {
			"dispatch_authorized" =>
				Ok(AuthorizeProviderDispatchOutcome::Fresh(FreshProviderDispatchFence {
					attempt_id: prepared.attempt_id,
					attempt_revision: mutation.revision,
					process_generation_id: process_generation_id.clone(),
					process_generation_revision,
					authorized_at_micros: mutation.recorded_at_micros,
				})),
			"replayed" => Ok(AuthorizeProviderDispatchOutcome::Replayed(mutation)),
			code => Ok(AuthorizeProviderDispatchOutcome::Rejected {
				rejection: parse_rejection(code)?,
				actual: mutation,
			}),
		}
	}

	/// Cancel one prepared attempt before dispatch authorization.
	pub async fn cancel_provider_attempt(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		self.provider_attempt_revision_transition(
			"SELECT result_code,revision,state::text,updated_at_micros \
			 FROM decodex.cancel_provider_attempt_exact($1::text::uuid,$2)",
			attempt_id,
			expected_revision,
			None,
			&["canceled"],
		)
		.await
	}

	/// Preserve an authorized attempt as unknown without claiming non-submission.
	pub async fn mark_provider_attempt_unknown(
		&self,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
		reason: ProviderAttemptUnknownReason,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		if reason == ProviderAttemptUnknownReason::RestoreProjection {
			return Err(StoreError::InvalidInput(
				"restore projection is not a live ProviderAttempt transition",
			));
		}
		self.provider_attempt_revision_transition(
			"SELECT result_code,revision,state::text,updated_at_micros \
			 FROM decodex.mark_provider_attempt_unknown_exact(\
			 $1::text::uuid,$2,$3::text::decodex.provider_attempt_unknown_reason)",
			attempt_id,
			expected_revision,
			Some(reason.as_sql()),
			&["unknown"],
		)
		.await
	}

	/// Resolve one authorized or unknown attempt only from exact positive evidence.
	pub async fn record_provider_attempt_positive_evidence(
		&self,
		expected_revision: i64,
		evidence: &ProviderPositiveEvidence,
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProviderAttempt revision must be positive"));
		}
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT result_code,revision,state::text,observed_at_micros \
				 FROM decodex.record_provider_attempt_positive_evidence_exact(\
				 $1::text::uuid,$2,$3::text::uuid,$4::text::uuid,\
				 $5::text::decodex.provider_attempt_evidence_source,\
				 $6::text::decodex.provider_attempt_terminal_outcome,\
				 $7,$8,$9,$10,$11)",
				&[
					&evidence.attempt_id.as_str(),
					&expected_revision,
					&evidence.evidence_id.as_str(),
					&evidence.request_id.as_str(),
					&evidence.source.as_sql(),
					&evidence.outcome.as_sql(),
					&evidence.provider_key.as_str(),
					&evidence.provider_receipt_id,
					&evidence.provider_thread_id,
					&evidence.provider_turn_id,
					&evidence.witness_digest,
				],
			)
			.await?;
		let result_code: &str = row.get(0);
		let mutation = parse_mutation(&row, 1, 2, 3)?;
		if result_code == evidence.outcome.as_sql() {
			Ok(ProviderAttemptMutationOutcome::Applied(mutation))
		} else if result_code == "replayed" {
			Ok(ProviderAttemptMutationOutcome::Replayed(mutation))
		} else {
			Ok(ProviderAttemptMutationOutcome::Rejected {
				rejection: parse_rejection(result_code)?,
				actual: mutation,
			})
		}
	}

	/// Project every present prepared or authorized row to unknown under the restore gate.
	pub async fn project_provider_attempts_after_supervisor_loss(&self) -> Result<u64, StoreError> {
		let changed: i64 = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT decodex.project_provider_attempts_after_supervisor_loss_exact()",
				&[],
			)
			.await?
			.get(0);
		u64::try_from(changed)
			.map_err(|_| StoreError::Incompatible("negative attempt projection count".into()))
	}

	/// Read one bounded exact page for sole-writer reconciliation.
	///
	/// Returned provider keys are redacted by `Debug`. The service must not publish them.
	pub async fn read_provider_attempt_page(
		&self,
		account_id: Option<&AccountId>,
		state: Option<ProviderAttemptState>,
		after_attempt_id: Option<&ProviderAttemptId>,
		limit: u16,
	) -> Result<Vec<ProviderAttempt>, StoreError> {
		self.read_provider_attempts(None, account_id, state, after_attempt_id, limit).await
	}

	/// Read one exact attempt for positive reconciliation.
	pub async fn read_provider_attempt(
		&self,
		attempt_id: &ProviderAttemptId,
	) -> Result<Option<ProviderAttempt>, StoreError> {
		let mut attempts =
			self.read_provider_attempts(Some(attempt_id), None, None, None, 1).await?;
		Ok(attempts.pop())
	}

	async fn read_provider_attempts(
		&self,
		attempt_id: Option<&ProviderAttemptId>,
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
		let attempt_id = attempt_id.map(ProviderAttemptId::as_str);
		let account_id = account_id.map(AccountId::as_str);
		let state = state.map(ProviderAttemptState::as_sql);
		let after_attempt_id = after_attempt_id.map(ProviderAttemptId::as_str);
		let limit = i64::from(limit);
		let rows = self
			.pool()
			.get()
			.await?
			.query(
				"SELECT attempt_id::text,consumer_kind::text,conversation_id::text,\
				 turn_id::text,managed_run_id::text,managed_run_revision,\
				 managed_execution_id::text,continuation_plan_id::text,\
				 routing_decision_id::text,accepted_runtime_session_id::text,\
				 accepted_runtime_session_revision,selected_account_id::text,\
				 process_generation_id::text,process_generation_revision,\
				 process_execution_epoch_id::text,request_id::text,request_digest,\
				 provider_idempotency_key,provider_correlation_key,\
				 predecessor_attempt_id::text,duplicate_risk_ack_digest,state::text,\
				 unknown_reason::text,terminal_evidence_id::text,revision,\
				 created_at_micros,updated_at_micros \
				 FROM decodex.read_provider_attempts_exact(\
				 $1::text::uuid,$2::text::uuid,\
				 $3::text::decodex.provider_attempt_state,$4::text::uuid,$5)",
				&[&attempt_id, &account_id, &state, &after_attempt_id, &limit],
			)
			.await?;
		rows.into_iter().map(parse_attempt).collect()
	}

	async fn provider_attempt_revision_transition(
		&self,
		statement: &str,
		attempt_id: &ProviderAttemptId,
		expected_revision: i64,
		reason: Option<&str>,
		applied_codes: &[&str],
	) -> Result<ProviderAttemptMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProviderAttempt revision must be positive"));
		}
		let client = self.pool().get().await?;
		let row = match reason {
			Some(reason) =>
				client
					.query_one(statement, &[&attempt_id.as_str(), &expected_revision, &reason])
					.await?,
			None =>
				client.query_one(statement, &[&attempt_id.as_str(), &expected_revision]).await?,
		};
		let result_code: &str = row.get(0);
		let mutation = parse_mutation(&row, 1, 2, 3)?;
		if applied_codes.contains(&result_code) {
			Ok(ProviderAttemptMutationOutcome::Applied(mutation))
		} else if result_code == "replayed" {
			Ok(ProviderAttemptMutationOutcome::Replayed(mutation))
		} else {
			Ok(ProviderAttemptMutationOutcome::Rejected {
				rejection: parse_rejection(result_code)?,
				actual: mutation,
			})
		}
	}
}

type ConsumerParameters<'a> =
	(Option<&'a str>, Option<&'a str>, Option<&'a str>, Option<i64>, Option<&'a str>);

fn consumer_parameters(consumer: &ProviderAttemptConsumer) -> ConsumerParameters<'_> {
	match consumer {
		ProviderAttemptConsumer::ConversationTurn { conversation_id, turn_id } =>
			(Some(conversation_id.as_str()), Some(turn_id.as_str()), None, None, None),
		ProviderAttemptConsumer::ManagedRunExecution {
			managed_run_id,
			managed_run_revision,
			execution_id,
		} => (
			None,
			None,
			Some(managed_run_id.as_str()),
			Some(*managed_run_revision),
			Some(execution_id.as_str()),
		),
	}
}

fn duplicate_risk_parameters(risk: &ProviderDuplicateRisk) -> (Option<&str>, Option<&str>) {
	match risk {
		ProviderDuplicateRisk::OriginalIntent => (None, None),
		ProviderDuplicateRisk::AcknowledgedSuccessor {
			predecessor_attempt_id,
			acknowledgement_digest,
		} => (Some(predecessor_attempt_id.as_str()), Some(acknowledgement_digest)),
	}
}

fn parse_mutation(
	row: &tokio_postgres::Row,
	revision_index: usize,
	state_index: usize,
	time_index: usize,
) -> Result<ProviderAttemptMutation, StoreError> {
	let revision: i64 = row.get(revision_index);
	let recorded_at_micros: i64 = row.get(time_index);
	if revision < 0 || recorded_at_micros < 0 {
		return Err(incompatible_value("ProviderAttempt mutation coordinate"));
	}
	Ok(ProviderAttemptMutation {
		revision,
		state: parse_state(row.get(state_index))?,
		recorded_at_micros,
	})
}

fn parse_attempt(row: tokio_postgres::Row) -> Result<ProviderAttempt, StoreError> {
	let attempt_id = ProviderAttemptId::new(row.get::<_, String>(0))
		.map_err(|_| incompatible_value("ProviderAttempt identity"))?;
	let consumer = parse_consumer(&row)?;
	let continuation_plan_id = row.get::<_, String>(7);
	let routing_decision_id = row.get::<_, String>(8);
	if !is_canonical_uuid(&continuation_plan_id) || !is_canonical_uuid(&routing_decision_id) {
		return Err(incompatible_value(
			"ProviderAttempt Routing Decision/Continuation Plan identity",
		));
	}
	let runtime_session_id = RuntimeSessionId::new(row.get::<_, String>(9))
		.map_err(|_| incompatible_value("accepted RuntimeSession identity"))?;
	let runtime_session_revision: i64 = row.get(10);
	let account_id = AccountId::new(row.get::<_, String>(11))
		.map_err(|_| incompatible_value("selected account identity"))?;
	let process_generation_id = ProcessGenerationId::new(row.get::<_, String>(12))
		.map_err(|_| incompatible_value("bound ProcessGeneration identity"))?;
	let process_generation_revision: i64 = row.get(13);
	let process_execution_epoch_id = ProcessExecutionEpochId::new(row.get::<_, String>(14))
		.map_err(|_| incompatible_value("bound execution epoch identity"))?;
	let request_id = ProviderRequestId::new(row.get::<_, String>(15))
		.map_err(|_| incompatible_value("provider request identity"))?;
	let request_digest: String = row.get(16);
	let idempotency = row
		.get::<_, Option<String>>(17)
		.map(ProviderRequestKey::new)
		.transpose()
		.map_err(|_| incompatible_value("provider idempotency key"))?;
	let correlation = row
		.get::<_, Option<String>>(18)
		.map(ProviderRequestKey::new)
		.transpose()
		.map_err(|_| incompatible_value("provider correlation key"))?;
	let provider_keys = ProviderRequestKeys::new(idempotency, correlation)
		.map_err(|_| incompatible_value("provider request keys"))?;
	let duplicate_risk = parse_duplicate_risk(&row)?;
	let state = parse_state(row.get(21))?;
	let unknown_reason = row.get::<_, Option<&str>>(22).map(parse_unknown_reason).transpose()?;
	let terminal_evidence_id = row
		.get::<_, Option<String>>(23)
		.map(ProviderEvidenceId::new)
		.transpose()
		.map_err(|_| incompatible_value("terminal provider evidence identity"))?;
	let revision: i64 = row.get(24);
	let created_at_micros: i64 = row.get(25);
	let updated_at_micros: i64 = row.get(26);
	if runtime_session_revision <= 0
		|| process_generation_revision <= 0
		|| revision <= 0
		|| created_at_micros < 0
		|| updated_at_micros < created_at_micros
		|| !is_sha256(&request_digest)
		|| (state == ProviderAttemptState::Unknown) != unknown_reason.is_some()
		|| matches!(
			state,
			ProviderAttemptState::Succeeded
				| ProviderAttemptState::FailedDefinitive
				| ProviderAttemptState::NotSubmitted
		) != terminal_evidence_id.is_some()
	{
		return Err(incompatible_value("ProviderAttempt projection"));
	}

	Ok(ProviderAttempt {
		attempt_id,
		consumer,
		continuation_plan_id,
		routing_decision_id,
		runtime_session_id,
		runtime_session_revision,
		account_id,
		process_generation_id,
		process_generation_revision,
		process_execution_epoch_id,
		request_id,
		request_digest,
		provider_keys,
		duplicate_risk,
		state,
		unknown_reason,
		terminal_evidence_id,
		revision,
		created_at_micros,
		updated_at_micros,
	})
}

fn parse_consumer(row: &tokio_postgres::Row) -> Result<ProviderAttemptConsumer, StoreError> {
	Ok(match row.get::<_, &str>(1) {
		"conversation_turn" => ProviderAttemptConsumer::ConversationTurn {
			conversation_id: ConversationId::new(required_optional_text(
				row,
				2,
				"Conversation identity",
			)?)
			.map_err(|_| incompatible_value("Conversation identity"))?,
			turn_id: TurnId::new(required_optional_text(row, 3, "Turn identity")?)
				.map_err(|_| incompatible_value("Turn identity"))?,
		},
		"managed_run_execution" => ProviderAttemptConsumer::ManagedRunExecution {
			managed_run_id: ManagedRunId::new(required_optional_text(
				row,
				4,
				"ManagedRun identity",
			)?)
			.map_err(|_| incompatible_value("ManagedRun identity"))?,
			managed_run_revision: row
				.get::<_, Option<i64>>(5)
				.filter(|revision| *revision > 0)
				.ok_or_else(|| incompatible_value("ManagedRun revision"))?,
			execution_id: ManagedExecutionId::new(required_optional_text(
				row,
				6,
				"ManagedRun execution identity",
			)?)
			.map_err(|_| incompatible_value("ManagedRun execution identity"))?,
		},
		_ => return Err(incompatible_value("ProviderAttempt consumer kind")),
	})
}

fn parse_duplicate_risk(row: &tokio_postgres::Row) -> Result<ProviderDuplicateRisk, StoreError> {
	let predecessor = row
		.get::<_, Option<String>>(19)
		.map(ProviderAttemptId::new)
		.transpose()
		.map_err(|_| incompatible_value("predecessor ProviderAttempt identity"))?;
	let acknowledgement = row.get::<_, Option<String>>(20);
	Ok(match (predecessor, acknowledgement) {
		(None, None) => ProviderDuplicateRisk::OriginalIntent,
		(Some(predecessor_attempt_id), Some(acknowledgement_digest))
			if is_sha256(&acknowledgement_digest) =>
			ProviderDuplicateRisk::AcknowledgedSuccessor {
				predecessor_attempt_id,
				acknowledgement_digest,
			},
		_ => return Err(incompatible_value("duplicate-risk acknowledgement")),
	})
}

fn required_optional_text(
	row: &tokio_postgres::Row,
	index: usize,
	name: &'static str,
) -> Result<String, StoreError> {
	row.get::<_, Option<String>>(index).ok_or_else(|| incompatible_value(name))
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
		_ => Err(incompatible_value("ProviderAttempt state")),
	}
}

fn parse_unknown_reason(value: &str) -> Result<ProviderAttemptUnknownReason, StoreError> {
	match value {
		"supervision_lost" => Ok(ProviderAttemptUnknownReason::SupervisionLost),
		"dispatch_outcome_unavailable" =>
			Ok(ProviderAttemptUnknownReason::DispatchOutcomeUnavailable),
		"restore_projection" => Ok(ProviderAttemptUnknownReason::RestoreProjection),
		_ => Err(incompatible_value("ProviderAttempt unknown reason")),
	}
}

fn parse_rejection(value: &str) -> Result<ProviderAttemptRejection, StoreError> {
	match value {
		"identity_conflict" => Ok(ProviderAttemptRejection::IdentityConflict),
		"authority_unavailable" => Ok(ProviderAttemptRejection::AuthorityUnavailable),
		"generation_unavailable" => Ok(ProviderAttemptRejection::GenerationUnavailable),
		"consumer_unavailable" => Ok(ProviderAttemptRejection::ConsumerUnavailable),
		"invalid_input" => Ok(ProviderAttemptRejection::InvalidInput),
		"attempt_missing" => Ok(ProviderAttemptRejection::AttemptMissing),
		"stale_attempt" => Ok(ProviderAttemptRejection::StaleAttempt),
		"evidence_conflict" => Ok(ProviderAttemptRejection::EvidenceConflict),
		"invalid_evidence" => Ok(ProviderAttemptRejection::InvalidEvidence),
		"evidence_mismatch" => Ok(ProviderAttemptRejection::EvidenceMismatch),
		_ => Err(incompatible_value("ProviderAttempt result code")),
	}
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

fn is_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn incompatible_value(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
