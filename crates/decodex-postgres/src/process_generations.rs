//! Least-privilege PostgreSQL capability for the sole ProcessGeneration writer.
//!
//! The runtime role can execute these closed functions. It cannot read or write the underlying
//! relations. A replayed pre-spawn fence never returns launch permission.

use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, BoundProcessGeneration, CredentialBinding,
	CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion,
	ProcessAuthorityLossReason, ProcessBootIdentity, ProcessControlKind, ProcessDeathEvidence,
	ProcessDeathEvidenceId, ProcessExecutionEpochId, ProcessGeneration,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationIntent,
	ProcessGenerationState, ProcessIdentity, ProcessIsolationKind, ProcessRunnerIdentity,
	ProcessStartIdentity, ProviderIdentity,
};

use crate::{FreshQuickTaskProcessGeneration, PostgresStore, StoreError};

const PREPARE_BOUND_PROCESS_GENERATION_SQL: &str = "SELECT result_code,revision,state::text,created_at_micros,updated_at_micros \
	 FROM decodex.prepare_process_generation_exact(\
	 $1::text::uuid,$2::text::uuid,$3::text::uuid,$4,$5,$6,\
	 $7::text::decodex.process_generation_control_kind,\
	 $8::text::decodex.process_generation_isolation_kind,$9,$10,$11,$12,\
	 $13::text::uuid,$14::text::decodex.account_provider_kind,$15,$16,\
	 $17,$18::text::uuid,$19::text::uuid,$20,$21,$22::text::uuid,$23,\
	 $24::text::uuid,$25,$26::text::uuid,$27,$28::text::uuid,$29::text::uuid)";
const READ_BOUND_PROCESS_GENERATIONS_SQL: &str = "SELECT generation_id::text,account_id::text,execution_epoch_id::text,\
	 runner_identity,intended_boot_id,control_kind::text,isolation_kind::text,\
	 bound_boot_id,process_id,process_start_id,process_group_id,session_id,\
	 state::text,revision,authority_loss_reason::text,death_evidence_id::text,\
	 created_at_micros,updated_at_micros,initial_account_revision,\
	 credential_store_schema_version,credential_version,credential_fingerprint,\
	 credential_writer_operation_id::text,provider_kind::text,provider_account_id,\
	 refresh_callback_profile_sha256 \
	 FROM decodex.read_process_generations_exact(\
	 $1::text::uuid,$2,$3::text::uuid,$4)";

/// Newly committed pre-spawn authority. Durable replay cannot construct this value.
#[derive(Debug, Eq, PartialEq)]
pub struct FreshProcessGenerationFence {
	generation_id: ProcessGenerationId,
	revision: i64,
	fenced_at_micros: i64,
}
impl FreshProcessGenerationFence {
	/// Return the exact generation whose intent committed before spawn.
	pub fn generation_id(&self) -> &ProcessGenerationId {
		&self.generation_id
	}

	/// Return the committed starting revision.
	pub const fn revision(&self) -> i64 {
		self.revision
	}

	/// Return the PostgreSQL-authored fence time.
	pub const fn fenced_at_micros(&self) -> i64 {
		self.fenced_at_micros
	}
}

/// One exact durable mutation projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGenerationMutation {
	/// Current durable revision.
	pub revision: i64,
	/// Current durable state.
	pub state: ProcessGenerationState,
	/// PostgreSQL-authored transition or observation time.
	pub recorded_at_micros: i64,
}

/// Closed rejection from the ProcessGeneration authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGenerationRejection {
	/// The generation identity was already assigned to different immutable intent.
	IdentityConflict,
	/// The exact Quick Task receipt, lineage, or active revision-1 Turn is unavailable.
	QuickTaskAuthorityUnavailable,
	/// The external execution epoch or its authorization digest is not active.
	RestoreAuthorityUnavailable,
	/// The requested account does not exist.
	AccountMissing,
	/// One unresolved generation already quarantines this account.
	AccountQuarantined,
	/// Account lifecycle/store facts were absent, stale, disabled, or unsettled.
	AccountLifecycleUnready,
	/// Exact generated/live refresh callback capability was not attested.
	CallbackCapabilityUnready,
	/// The exact generation does not exist.
	GenerationMissing,
	/// The supplied revision or required durable state is stale.
	StaleGeneration,
	/// A process identity was already bound with different exact facts.
	ProcessIdentityConflict,
	/// The supplied process identity is not one exact session leader.
	InvalidProcessIdentity,
	/// The evidence identity was already assigned to different facts.
	EvidenceConflict,
	/// The positive evidence shape is malformed.
	InvalidEvidence,
	/// The evidence does not match the exact durable generation.
	EvidenceMismatch,
}

/// Result of the durable pre-spawn fence.
#[derive(Debug, Eq, PartialEq)]
pub enum PrepareProcessGenerationOutcome {
	/// Intent committed for the first time and can authorize exactly one spawn attempt.
	Fresh(FreshProcessGenerationFence),
	/// Intent was already durable. This is readback only and cannot authorize another spawn.
	Replayed(ProcessGenerationMutation),
	/// PostgreSQL rejected the fence.
	Rejected {
		/// Stable rejection.
		rejection: ProcessGenerationRejection,
		/// Current projection when one exists.
		actual: ProcessGenerationMutation,
	},
}

/// Result of a post-fence ProcessGeneration transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessGenerationMutationOutcome {
	/// The requested transition committed.
	Applied(ProcessGenerationMutation),
	/// The same exact transition was already durable.
	Replayed(ProcessGenerationMutation),
	/// PostgreSQL rejected the transition.
	Rejected {
		/// Stable rejection.
		rejection: ProcessGenerationRejection,
		/// Current projection when one exists.
		actual: ProcessGenerationMutation,
	},
}

impl PostgresStore {
	/// Persist generation intent before process creation can become externally effective.
	pub async fn prepare_process_generation(
		&self,
		_intent: &ProcessGenerationIntent,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		Err(StoreError::Incompatible(
			"V27 ProcessGeneration preparation requires an exact account binding".into(),
		))
	}

	/// Persist generation intent and immutable account/store/provider/callback facts before spawn.
	pub async fn prepare_bound_process_generation(
		&self,
		intent: &ProcessGenerationIntent,
		binding: &ProcessGenerationAccountBinding,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		self.prepare_bound_process_generation_inner(intent, binding, None, None).await
	}

	/// Persist one Quick Task generation while re-locking its exact active revision-1 Turn.
	pub async fn prepare_quick_task_bound_process_generation(
		&self,
		intent: &ProcessGenerationIntent,
		binding: &ProcessGenerationAccountBinding,
		admission: FreshQuickTaskProcessGeneration,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		if admission.generation_id() != &intent.generation_id {
			return Err(StoreError::InvalidInput(
				"Quick Task admission and ProcessGeneration identity differ",
			));
		}
		self.prepare_bound_process_generation_inner(intent, binding, None, Some(&admission)).await
	}

	/// Persist one generation authorized only by an already-started Reset Card claim.
	pub async fn prepare_reset_reconciliation_process_generation(
		&self,
		intent: &ProcessGenerationIntent,
		binding: &ProcessGenerationAccountBinding,
		outbox_id: i64,
		worker_id: &str,
		claim_token: &str,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		if outbox_id < 1 {
			return Err(StoreError::InvalidInput("reset-card outbox identity must be positive"));
		}
		self.prepare_bound_process_generation_inner(
			intent,
			binding,
			Some((outbox_id, worker_id, claim_token)),
			None,
		)
		.await
	}

	async fn prepare_bound_process_generation_inner(
		&self,
		intent: &ProcessGenerationIntent,
		binding: &ProcessGenerationAccountBinding,
		reconciliation: Option<(i64, &str, &str)>,
		quick_task: Option<&FreshQuickTaskProcessGeneration>,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		let client = self.pool().get().await?;
		let credential_version = i64::try_from(binding.credential.version.get()).map_err(|_| {
			StoreError::InvalidInput("credential version overflows PostgreSQL bigint")
		})?;
		let store_schema = i32::from(binding.credential.schema_version.get());
		let (outbox_id, worker_id, claim_token) =
			reconciliation.map_or((None, None, None), |(outbox_id, worker_id, claim_token)| {
				(Some(outbox_id), Some(worker_id), Some(claim_token))
			});
		let (
			quick_task_protocol,
			quick_task_key,
			quick_task_conversation_id,
			quick_task_conversation_revision,
			quick_task_runtime_session_id,
			quick_task_runtime_session_revision,
			quick_task_turn_id,
			quick_task_turn_revision,
			quick_task_continuation_plan_id,
			quick_task_routing_decision_id,
		) = quick_task.map_or(
			(None, None, None, None, None, None, None, None, None, None),
			|admission| {
				let request = &admission.readback().request;
				(
					Some(admission.protocol_version()),
					Some(admission.idempotency_key()),
					Some(request.conversation_id.as_str()),
					Some(request.expected_conversation_revision),
					Some(request.runtime_session_id.as_str()),
					Some(request.expected_runtime_session_revision),
					Some(request.turn_id.as_str()),
					Some(request.expected_turn_revision),
					Some(request.continuation_plan_id.as_str()),
					Some(request.routing_decision_id.as_str()),
				)
			},
		);
		let row = client
			.query_one(
				PREPARE_BOUND_PROCESS_GENERATION_SQL,
				&[
					&intent.generation_id.as_str(),
					&intent.account_id.as_str(),
					&intent.execution_authorization.epoch_id.as_str(),
					&intent.execution_authorization.authorization_digest,
					&intent.runner_identity.as_str(),
					&intent.intended_boot_id.as_str(),
					&intent.control_kind.as_sql(),
					&intent.isolation_kind.as_sql(),
					&binding.account_revision,
					&store_schema,
					&credential_version,
					&binding.credential.fingerprint.as_str(),
					&binding.credential.writer_operation_id.as_str(),
					&provider_text(binding.credential.provider.provider()),
					&binding.credential.provider.account_id(),
					&binding.refresh_callback_profile_sha256,
					&outbox_id,
					&worker_id,
					&claim_token,
					&quick_task_protocol,
					&quick_task_key,
					&quick_task_conversation_id,
					&quick_task_conversation_revision,
					&quick_task_runtime_session_id,
					&quick_task_runtime_session_revision,
					&quick_task_turn_id,
					&quick_task_turn_revision,
					&quick_task_continuation_plan_id,
					&quick_task_routing_decision_id,
				],
			)
			.await?;
		let result_code: &str = row.get(0);
		let mutation = parse_mutation(&row, 1, 2, 4)?;
		match result_code {
			"prepared" => Ok(PrepareProcessGenerationOutcome::Fresh(FreshProcessGenerationFence {
				generation_id: intent.generation_id.clone(),
				revision: mutation.revision,
				fenced_at_micros: row.get(3),
			})),
			"replayed" => Ok(PrepareProcessGenerationOutcome::Replayed(mutation)),
			code => Ok(PrepareProcessGenerationOutcome::Rejected {
				rejection: parse_rejection(code)?,
				actual: mutation,
			}),
		}
	}

	/// Read exact diagnostics with the immutable V27 account binding when present.
	pub async fn read_bound_process_generations(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		limit: u16,
	) -> Result<Vec<BoundProcessGeneration>, StoreError> {
		self.read_bound_process_generation_page(account_id, include_dead, None, limit).await
	}

	/// Read one bounded page with complete immutable V27 account bindings.
	pub async fn read_bound_process_generation_page(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		after_generation_id: Option<&ProcessGenerationId>,
		limit: u16,
	) -> Result<Vec<BoundProcessGeneration>, StoreError> {
		if !(1..=256).contains(&limit) {
			return Err(StoreError::InvalidInput(
				"ProcessGeneration diagnostic limit must be between 1 and 256",
			));
		}
		let account_id = account_id.map(AccountId::as_str);
		let after_generation_id = after_generation_id.map(ProcessGenerationId::as_str);
		let limit = i64::from(limit);
		let rows = self
			.pool()
			.get()
			.await?
			.query(
				READ_BOUND_PROCESS_GENERATIONS_SQL,
				&[&account_id, &include_dead, &after_generation_id, &limit],
			)
			.await?;
		rows.into_iter().map(parse_bound_generation).collect()
	}

	/// Bind the exact process, group, session, boot, and start identity immediately after spawn.
	pub async fn bind_process_generation_identity(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		identity: &ProcessIdentity,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProcessGeneration revision must be positive"));
		}
		let process_id = i64::from(identity.process_id);
		let process_group_id = i64::from(identity.process_group_id);
		let session_id = i64::from(identity.session_id);
		self.process_generation_transition(
			"SELECT result_code,revision,state::text,updated_at_micros \
			 FROM decodex.bind_process_generation_identity_exact(\
			 $1::text::uuid,$2,$3,$4,$5,$6,$7)",
			&[
				&generation_id.as_str(),
				&expected_revision,
				&identity.boot_id.as_str(),
				&process_id,
				&identity.process_start_id.as_str(),
				&process_group_id,
				&session_id,
			],
			&["bound"],
		)
		.await
	}

	/// Mark one identity-bound generation ready after application initialization succeeds.
	pub async fn mark_process_generation_ready(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		self.revision_transition(
			"SELECT result_code,revision,state::text,updated_at_micros \
			 FROM decodex.mark_process_generation_ready_exact($1::text::uuid,$2)",
			generation_id,
			expected_revision,
			&["ready"],
		)
		.await
	}

	/// Mark one identity-bound generation as stopping before exact-identity signaling.
	pub async fn mark_process_generation_stopping(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		self.revision_transition(
			"SELECT result_code,revision,state::text,updated_at_micros \
			 FROM decodex.mark_process_generation_stopping_exact($1::text::uuid,$2)",
			generation_id,
			expected_revision,
			&["stopping"],
		)
		.await
	}

	/// Quarantine one unresolved generation after exact supervision authority is lost.
	pub async fn mark_process_generation_death_unknown(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		reason: ProcessAuthorityLossReason,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProcessGeneration revision must be positive"));
		}
		self.process_generation_transition(
			"SELECT result_code,revision,state::text,updated_at_micros \
			 FROM decodex.mark_process_generation_death_unknown_exact(\
			 $1::text::uuid,$2,$3::text::decodex.process_generation_loss_reason)",
			&[&generation_id.as_str(), &expected_revision, &reason.as_sql()],
			&["death_unknown"],
		)
		.await
	}

	/// Persist one positive generation-bound death receipt and transition to dead.
	pub async fn record_process_generation_death(
		&self,
		expected_revision: i64,
		evidence: &ProcessDeathEvidence,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProcessGeneration revision must be positive"));
		}
		let (process_id, process_start_id, process_group_id, session_id) =
			optional_identity_parameters(evidence.process_identity.as_ref());
		self.process_generation_transition(
			"SELECT result_code,revision,state::text,observed_at_micros \
			 FROM decodex.record_process_generation_death_exact(\
			 $1::text::uuid,$2,$3::text::uuid,\
			 $4::text::decodex.process_generation_death_evidence_kind,\
			 $5,$6,$7,$8,$9,$10)",
			&[
				&evidence.generation_id.as_str(),
				&expected_revision,
				&evidence.evidence_id.as_str(),
				&evidence.kind.as_sql(),
				&evidence.observed_boot_id.as_str(),
				&process_id,
				&process_start_id,
				&process_group_id,
				&session_id,
				&evidence.witness_digest,
			],
			&["dead"],
		)
		.await
	}

	/// Project every restored nonterminal generation to `death_unknown`.
	pub async fn project_process_generations_after_supervisor_loss(
		&self,
	) -> Result<u64, StoreError> {
		let changed: i64 = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT decodex.project_process_generations_after_supervisor_loss_exact()",
				&[],
			)
			.await?
			.get(0);
		u64::try_from(changed)
			.map_err(|_| StoreError::Incompatible("negative generation projection count".into()))
	}

	/// Read bounded exact diagnostics. This read cannot return execution-epoch authorization.
	pub async fn read_process_generations(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		limit: u16,
	) -> Result<Vec<ProcessGeneration>, StoreError> {
		self.read_process_generation_page(account_id, include_dead, None, limit).await
	}

	/// Read one bounded reconciliation page after an exact generation identity.
	pub async fn read_process_generation_page(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		after_generation_id: Option<&ProcessGenerationId>,
		limit: u16,
	) -> Result<Vec<ProcessGeneration>, StoreError> {
		if !(1..=256).contains(&limit) {
			return Err(StoreError::InvalidInput(
				"ProcessGeneration diagnostic limit must be between 1 and 256",
			));
		}
		let account_id = account_id.map(|account_id| account_id.as_str());
		let after_generation_id = after_generation_id.map(|generation_id| generation_id.as_str());
		let limit = i64::from(limit);
		let rows = self
			.pool()
			.get()
			.await?
			.query(
				"SELECT generation_id::text,account_id::text,execution_epoch_id::text,\
				 runner_identity,intended_boot_id,control_kind::text,isolation_kind::text,\
				 bound_boot_id,process_id,process_start_id,process_group_id,session_id,\
				 state::text,revision,authority_loss_reason::text,death_evidence_id::text,\
				 created_at_micros,updated_at_micros \
				 FROM decodex.read_process_generations_exact(\
				 $1::text::uuid,$2,$3::text::uuid,$4)",
				&[&account_id, &include_dead, &after_generation_id, &limit],
			)
			.await?;
		rows.into_iter().map(parse_generation).collect()
	}

	async fn revision_transition(
		&self,
		statement: &str,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		applied_codes: &[&str],
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProcessGeneration revision must be positive"));
		}
		self.process_generation_transition(
			statement,
			&[&generation_id.as_str(), &expected_revision],
			applied_codes,
		)
		.await
	}

	async fn process_generation_transition(
		&self,
		statement: &str,
		parameters: &[&(dyn tokio_postgres::types::ToSql + Sync)],
		applied_codes: &[&str],
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		let row = self.pool().get().await?.query_one(statement, parameters).await?;
		let result_code: &str = row.get(0);
		let mutation = parse_mutation(&row, 1, 2, 3)?;
		if applied_codes.contains(&result_code) {
			Ok(ProcessGenerationMutationOutcome::Applied(mutation))
		} else if result_code == "replayed" {
			Ok(ProcessGenerationMutationOutcome::Replayed(mutation))
		} else {
			Ok(ProcessGenerationMutationOutcome::Rejected {
				rejection: parse_rejection(result_code)?,
				actual: mutation,
			})
		}
	}
}

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_account_bound_process_generation_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 2] =
		[PREPARE_BOUND_PROCESS_GENERATION_SQL, READ_BOUND_PROCESS_GENERATIONS_SQL];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

type OptionalIdentityParameters<'a> = (Option<i64>, Option<&'a str>, Option<i64>, Option<i64>);

fn optional_identity_parameters(
	identity: Option<&ProcessIdentity>,
) -> OptionalIdentityParameters<'_> {
	identity.map_or((None, None, None, None), |identity| {
		(
			Some(i64::from(identity.process_id)),
			Some(identity.process_start_id.as_str()),
			Some(i64::from(identity.process_group_id)),
			Some(i64::from(identity.session_id)),
		)
	})
}

fn parse_mutation(
	row: &tokio_postgres::Row,
	revision_index: usize,
	state_index: usize,
	time_index: usize,
) -> Result<ProcessGenerationMutation, StoreError> {
	let revision: i64 = row.get(revision_index);
	let recorded_at_micros: i64 = row.get(time_index);
	if revision < 0 || recorded_at_micros < 0 {
		return Err(incompatible_value("ProcessGeneration mutation coordinate"));
	}
	Ok(ProcessGenerationMutation {
		revision,
		state: parse_state(row.get(state_index))?,
		recorded_at_micros,
	})
}

fn parse_generation(row: tokio_postgres::Row) -> Result<ProcessGeneration, StoreError> {
	let generation_id = ProcessGenerationId::new(row.get::<_, String>(0))
		.map_err(|_| incompatible_value("generation identity"))?;
	let account_id = AccountId::new(row.get::<_, String>(1))
		.map_err(|_| incompatible_value("generation account identity"))?;
	let execution_epoch_id = ProcessExecutionEpochId::new(row.get::<_, String>(2))
		.map_err(|_| incompatible_value("execution epoch identity"))?;
	let runner_identity = ProcessRunnerIdentity::new(row.get::<_, String>(3))
		.map_err(|_| incompatible_value("runner identity"))?;
	let intended_boot_id = ProcessBootIdentity::new(row.get::<_, String>(4))
		.map_err(|_| incompatible_value("intended boot identity"))?;
	let control_kind = parse_control_kind(row.get(5))?;
	let isolation_kind = parse_isolation_kind(row.get(6))?;
	let bound_boot_id = row
		.get::<_, Option<String>>(7)
		.map(ProcessBootIdentity::new)
		.transpose()
		.map_err(|_| incompatible_value("bound boot identity"))?;
	let process_id = optional_u32(row.get(8), "process identity")?;
	let process_start_id = row
		.get::<_, Option<String>>(9)
		.map(ProcessStartIdentity::new)
		.transpose()
		.map_err(|_| incompatible_value("process-start identity"))?;
	let process_group_id = optional_u32(row.get(10), "process-group identity")?;
	let session_id = optional_u32(row.get(11), "session identity")?;
	let process_identity = match (
		bound_boot_id.as_ref(),
		process_id,
		process_start_id.as_ref(),
		process_group_id,
		session_id,
	) {
		(None, None, None, None, None) => None,
		(Some(boot_id), Some(process_id), Some(start_id), Some(group_id), Some(session_id)) =>
			Some(
				ProcessIdentity::new(
					boot_id.clone(),
					process_id,
					start_id.clone(),
					group_id,
					session_id,
				)
				.map_err(|_| incompatible_value("exact process identity"))?,
			),
		_ => return Err(incompatible_value("partial process identity")),
	};
	let state = parse_state(row.get(12))?;
	let revision: i64 = row.get(13);
	let authority_loss_reason =
		row.get::<_, Option<&str>>(14).map(parse_loss_reason).transpose()?;
	let death_evidence_id = row
		.get::<_, Option<String>>(15)
		.map(ProcessDeathEvidenceId::new)
		.transpose()
		.map_err(|_| incompatible_value("death evidence identity"))?;
	let created_at_micros: i64 = row.get(16);
	let updated_at_micros: i64 = row.get(17);
	if revision <= 0
		|| created_at_micros < 0
		|| updated_at_micros < created_at_micros
		|| process_identity
			.as_ref()
			.is_some_and(|identity| identity.boot_id.as_str() != intended_boot_id.as_str())
		|| (matches!(state, ProcessGenerationState::Ready | ProcessGenerationState::Stopping)
			&& process_identity.is_none())
		|| (state == ProcessGenerationState::DeathUnknown) != authority_loss_reason.is_some()
		|| (state == ProcessGenerationState::Dead) != death_evidence_id.is_some()
	{
		return Err(incompatible_value("ProcessGeneration projection"));
	}
	Ok(ProcessGeneration {
		generation_id,
		account_id,
		execution_epoch_id,
		runner_identity,
		intended_boot_id,
		control_kind,
		isolation_kind,
		process_identity,
		state,
		authority_loss_reason,
		death_evidence_id,
		revision,
		created_at_micros,
		updated_at_micros,
	})
}

fn parse_bound_generation(row: tokio_postgres::Row) -> Result<BoundProcessGeneration, StoreError> {
	let account_binding = match (
		row.get::<_, Option<i64>>(18),
		row.get::<_, Option<i32>>(19),
		row.get::<_, Option<i64>>(20),
		row.get::<_, Option<String>>(21),
		row.get::<_, Option<String>>(22),
		row.get::<_, Option<&str>>(23),
		row.get::<_, Option<String>>(24),
		row.get::<_, Option<String>>(25),
	) {
		(None, None, None, None, None, None, None, None) => None,
		(
			Some(account_revision),
			Some(schema),
			Some(version),
			Some(fingerprint),
			Some(writer_operation_id),
			Some("chatgpt"),
			Some(provider_account_id),
			Some(callback),
		) => {
			let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)
				.map_err(|_| incompatible_value("generation provider identity"))?;
			let credential = CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::new(
					u16::try_from(schema)
						.map_err(|_| incompatible_value("generation credential schema"))?,
				)
				.map_err(|_| incompatible_value("generation credential schema"))?,
				version: CredentialVersion::new(
					u64::try_from(version)
						.map_err(|_| incompatible_value("generation credential version"))?,
				)
				.map_err(|_| incompatible_value("generation credential version"))?,
				fingerprint: CredentialFingerprint::new(fingerprint)
					.map_err(|_| incompatible_value("generation credential fingerprint"))?,
				provider,
				writer_operation_id: AccountOperationId::new(writer_operation_id)
					.map_err(|_| incompatible_value("generation credential writer"))?,
			};
			Some(
				ProcessGenerationAccountBinding::new(account_revision, credential, callback)
					.map_err(|_| incompatible_value("generation account binding"))?,
			)
		},
		_ => return Err(incompatible_value("partial generation account binding")),
	};
	let generation = parse_generation(row)?;
	Ok(BoundProcessGeneration { generation, account_binding })
}

fn parse_state(value: &str) -> Result<ProcessGenerationState, StoreError> {
	match value {
		"starting" => Ok(ProcessGenerationState::Starting),
		"ready" => Ok(ProcessGenerationState::Ready),
		"stopping" => Ok(ProcessGenerationState::Stopping),
		"dead" => Ok(ProcessGenerationState::Dead),
		"death_unknown" => Ok(ProcessGenerationState::DeathUnknown),
		_ => Err(incompatible_value("generation state")),
	}
}

fn parse_control_kind(value: &str) -> Result<ProcessControlKind, StoreError> {
	match value {
		"stdio_only_best_effort_eof" => Ok(ProcessControlKind::StdioOnlyBestEffortEof),
		"parent_death_signal_and_stdio_eof" => Ok(ProcessControlKind::ParentDeathSignalAndStdioEof),
		_ => Err(incompatible_value("generation control kind")),
	}
}

fn parse_isolation_kind(value: &str) -> Result<ProcessIsolationKind, StoreError> {
	match value {
		"session" => Ok(ProcessIsolationKind::Session),
		_ => Err(incompatible_value("generation isolation kind")),
	}
}

fn parse_loss_reason(value: &str) -> Result<ProcessAuthorityLossReason, StoreError> {
	match value {
		"supervisor_restarted" => Ok(ProcessAuthorityLossReason::SupervisorRestarted),
		"identity_persistence_failed" => Ok(ProcessAuthorityLossReason::IdentityPersistenceFailed),
		"readiness_persistence_failed" =>
			Ok(ProcessAuthorityLossReason::ReadinessPersistenceFailed),
		"termination_unproved" => Ok(ProcessAuthorityLossReason::TerminationUnproved),
		"control_authority_lost" => Ok(ProcessAuthorityLossReason::ControlAuthorityLost),
		_ => Err(incompatible_value("generation authority-loss reason")),
	}
}

fn parse_rejection(value: &str) -> Result<ProcessGenerationRejection, StoreError> {
	match value {
		"identity_conflict" => Ok(ProcessGenerationRejection::IdentityConflict),
		"quick_task_authority_unavailable" =>
			Ok(ProcessGenerationRejection::QuickTaskAuthorityUnavailable),
		"restore_authority_unavailable" =>
			Ok(ProcessGenerationRejection::RestoreAuthorityUnavailable),
		"account_missing" => Ok(ProcessGenerationRejection::AccountMissing),
		"account_quarantined" => Ok(ProcessGenerationRejection::AccountQuarantined),
		"account_lifecycle_unready" => Ok(ProcessGenerationRejection::AccountLifecycleUnready),
		"callback_capability_unready" => Ok(ProcessGenerationRejection::CallbackCapabilityUnready),
		"generation_missing" => Ok(ProcessGenerationRejection::GenerationMissing),
		"stale_generation" => Ok(ProcessGenerationRejection::StaleGeneration),
		"process_identity_conflict" => Ok(ProcessGenerationRejection::ProcessIdentityConflict),
		"invalid_process_identity" => Ok(ProcessGenerationRejection::InvalidProcessIdentity),
		"evidence_conflict" => Ok(ProcessGenerationRejection::EvidenceConflict),
		"invalid_evidence" => Ok(ProcessGenerationRejection::InvalidEvidence),
		"evidence_mismatch" => Ok(ProcessGenerationRejection::EvidenceMismatch),
		_ => Err(incompatible_value("ProcessGeneration result code")),
	}
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

fn optional_u32(value: Option<i64>, name: &'static str) -> Result<Option<u32>, StoreError> {
	value.map(u32::try_from).transpose().map_err(|_| incompatible_value(name))
}

fn incompatible_value(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
