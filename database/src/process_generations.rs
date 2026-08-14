//! Sole durable writer for local ProcessGeneration authority.

use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, BoundProcessGeneration, CredentialBinding,
	CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion,
	ProcessAuthorityLossReason, ProcessBootIdentity, ProcessControlKind, ProcessDeathEvidence,
	ProcessDeathEvidenceId, ProcessExecutionEpochId, ProcessGeneration,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationIntent,
	ProcessGenerationState, ProcessIdentity, ProcessIsolationKind, ProcessRunnerIdentity,
	ProcessStartIdentity, ProviderIdentity,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::{
	FreshQuickTaskProcessGeneration, SqliteStore, StoreError, account_lifecycle::sql_error,
	unix_micros,
};

#[derive(Debug, Eq, PartialEq)]
pub struct FreshProcessGenerationFence {
	generation_id: ProcessGenerationId,
	revision: i64,
	fenced_at_micros: i64,
}

impl FreshProcessGenerationFence {
	pub fn generation_id(&self) -> &ProcessGenerationId {
		&self.generation_id
	}

	pub const fn revision(&self) -> i64 {
		self.revision
	}

	pub const fn fenced_at_micros(&self) -> i64 {
		self.fenced_at_micros
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGenerationMutation {
	pub revision: i64,
	pub state: ProcessGenerationState,
	pub recorded_at_micros: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGenerationRejection {
	IdentityConflict,
	QuickTaskAuthorityUnavailable,
	RestoreAuthorityUnavailable,
	AccountMissing,
	AccountQuarantined,
	AccountLifecycleUnready,
	CallbackCapabilityUnready,
	GenerationMissing,
	StaleGeneration,
	ProcessIdentityConflict,
	InvalidProcessIdentity,
	EvidenceConflict,
	InvalidEvidence,
	EvidenceMismatch,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PrepareProcessGenerationOutcome {
	Fresh(FreshProcessGenerationFence),
	Replayed(ProcessGenerationMutation),
	Rejected { rejection: ProcessGenerationRejection, actual: ProcessGenerationMutation },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessGenerationMutationOutcome {
	Applied(ProcessGenerationMutation),
	Replayed(ProcessGenerationMutation),
	Rejected { rejection: ProcessGenerationRejection, actual: ProcessGenerationMutation },
}

impl SqliteStore {
	/// Persist one Quick Task generation intent before process creation.
	#[allow(clippy::too_many_lines)] // Keep one atomic generation-admission transaction together.
	pub async fn prepare_quick_task_bound_process_generation(
		&self,
		intent: &ProcessGenerationIntent,
		binding: &ProcessGenerationAccountBinding,
		admission: FreshQuickTaskProcessGeneration,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		if admission.generation_id() != &intent.generation_id
			|| intent.account_id != admission.readback().request.selected_account_id
		{
			return Err(StoreError::InvalidInput(
				"Quick Task admission and ProcessGeneration identity differ",
			));
		}
		let intent = intent.clone();
		let binding = binding.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let receipt_matches: bool = transaction
				.query_row(
					"SELECT EXISTS (
				   SELECT 1 FROM runtime_command_receipts
				   WHERE idempotency_key = ?1 AND request_sha256 = ?2
				     AND operation = 'prepare_quick_task_process_generation'
				     AND entity_id = ?3
				 )",
					params![
						admission.idempotency_key(),
						admission.request_sha256(),
						intent.generation_id.as_str()
					],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if !receipt_matches {
				return Ok(PrepareProcessGenerationOutcome::Rejected {
					rejection: ProcessGenerationRejection::QuickTaskAuthorityUnavailable,
					actual: empty_mutation(),
				});
			}
			if let Some(existing) = read_generation(&transaction, intent.generation_id.as_str())? {
				let same = existing.account_id == intent.account_id
					&& existing.execution_epoch_id == intent.execution_authorization.epoch_id
					&& existing.runner_identity == intent.runner_identity
					&& existing.intended_boot_id == intent.intended_boot_id
					&& existing.control_kind == intent.control_kind
					&& existing.isolation_kind == intent.isolation_kind;
				let mutation = mutation(&existing);
				transaction.commit().map_err(sql_error)?;
				return if same {
					Ok(PrepareProcessGenerationOutcome::Replayed(mutation))
				} else {
					Ok(PrepareProcessGenerationOutcome::Rejected {
						rejection: ProcessGenerationRejection::IdentityConflict,
						actual: mutation,
					})
				};
			}
			let authority = account_authority(&transaction, &intent.account_id, &binding)?;
			if let Some(rejection) = authority {
				return Ok(PrepareProcessGenerationOutcome::Rejected {
					rejection,
					actual: empty_mutation(),
				});
			}
			let quarantined: bool = transaction
				.query_row(
					"SELECT EXISTS (SELECT 1 FROM process_generations
				 WHERE account_id = ?1 AND state <> 'dead')",
					params![intent.account_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if quarantined {
				return Ok(PrepareProcessGenerationOutcome::Rejected {
					rejection: ProcessGenerationRejection::AccountQuarantined,
					actual: empty_mutation(),
				});
			}
			transaction
				.execute(
					"INSERT OR IGNORE INTO process_execution_epochs (
				   execution_epoch_id, authorization_sha256, created_at_micros
				 ) VALUES (?1, ?2, ?3)",
					params![
						intent.execution_authorization.epoch_id.as_str(),
						intent.execution_authorization.authorization_digest,
						unix_micros().map_err(StoreError::from)?,
					],
				)
				.map_err(sql_error)?;
			let epoch_digest: String = transaction
				.query_row(
					"SELECT authorization_sha256 FROM process_execution_epochs
				 WHERE execution_epoch_id = ?1",
					params![intent.execution_authorization.epoch_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if epoch_digest != intent.execution_authorization.authorization_digest {
				return Ok(PrepareProcessGenerationOutcome::Rejected {
					rejection: ProcessGenerationRejection::RestoreAuthorityUnavailable,
					actual: empty_mutation(),
				});
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let credential_version =
				i64::try_from(binding.credential.version.get()).map_err(|_| {
					StoreError::InvalidInput("credential version overflows SQLite integer")
				})?;
			transaction
				.execute(
					"INSERT INTO process_generations (
				   generation_id, account_id, runtime_session_id, execution_epoch_id,
				   runner_identity, intended_boot_id, control_kind, isolation_kind,
				   account_revision, credential_schema_version, credential_version,
				   credential_fingerprint, credential_writer_operation_id, provider,
				   provider_account_id, refresh_callback_profile_sha256,
				   quick_task_admission_key, state, revision, created_at_micros, updated_at_micros
				 ) VALUES (
				   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
				   ?14, ?15, ?16, ?17, 'starting', 1, ?18, ?18
				 )",
					params![
						intent.generation_id.as_str(),
						intent.account_id.as_str(),
						admission.readback().request.runtime_session_id.as_str(),
						intent.execution_authorization.epoch_id.as_str(),
						intent.runner_identity.as_str(),
						intent.intended_boot_id.as_str(),
						intent.control_kind.as_sql(),
						intent.isolation_kind.as_sql(),
						binding.account_revision,
						i64::from(binding.credential.schema_version.get()),
						credential_version,
						binding.credential.fingerprint.as_str(),
						binding.credential.writer_operation_id.as_str(),
						provider_text(binding.credential.provider.provider()),
						binding.credential.provider.account_id(),
						binding.refresh_callback_profile_sha256,
						admission.idempotency_key(),
						now,
					],
				)
				.map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(PrepareProcessGenerationOutcome::Fresh(FreshProcessGenerationFence {
				generation_id: intent.generation_id,
				revision: 1,
				fenced_at_micros: now,
			}))
		})
		.await
	}

	pub async fn read_bound_process_generations(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		limit: u16,
	) -> Result<Vec<BoundProcessGeneration>, StoreError> {
		self.read_bound_process_generation_page(account_id, include_dead, None, limit).await
	}

	pub async fn read_bound_process_generation_page(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		after_generation_id: Option<&ProcessGenerationId>,
		limit: u16,
	) -> Result<Vec<BoundProcessGeneration>, StoreError> {
		validate_limit(limit)?;
		let account_id = account_id.map(|value| value.as_str().to_owned());
		let after = after_generation_id.map(|value| value.as_str().to_owned());
		self.run(move |connection| {
			read_bound_page(
				connection,
				account_id.as_deref(),
				include_dead,
				after.as_deref(),
				limit,
			)
		})
		.await
	}

	pub async fn read_process_generation_page(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		after_generation_id: Option<&ProcessGenerationId>,
		limit: u16,
	) -> Result<Vec<ProcessGeneration>, StoreError> {
		Ok(self
			.read_bound_process_generation_page(
				account_id,
				include_dead,
				after_generation_id,
				limit,
			)
			.await?
			.into_iter()
			.map(|value| value.generation)
			.collect())
	}

	pub async fn bind_process_generation_identity(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		identity: &ProcessIdentity,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		if expected_revision <= 0
			|| identity.process_id != identity.process_group_id
			|| identity.process_id != identity.session_id
		{
			return Err(StoreError::InvalidInput("ProcessGeneration identity is invalid"));
		}
		let generation_id = generation_id.clone();
		let identity = identity.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let current = read_required_generation(&transaction, &generation_id)?;
			if current.revision == expected_revision.saturating_add(1)
				&& current.process_identity.as_ref() == Some(&identity)
			{
				let result = ProcessGenerationMutationOutcome::Replayed(mutation(&current));
				transaction.commit().map_err(sql_error)?;
				return Ok(result);
			}
			if current.revision != expected_revision
				|| current.state != ProcessGenerationState::Starting
				|| current.process_identity.is_some()
				|| identity.boot_id != current.intended_boot_id
			{
				return Ok(rejected(ProcessGenerationRejection::StaleGeneration, &current));
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let revision = expected_revision + 1;
			transaction
				.execute(
					"UPDATE process_generations SET bound_boot_id = ?1, process_id = ?2,
				   process_start_id = ?3, process_group_id = ?4, session_id = ?5,
				   revision = ?6, updated_at_micros = ?7 WHERE generation_id = ?8",
					params![
						identity.boot_id.as_str(),
						i64::from(identity.process_id),
						identity.process_start_id.as_str(),
						i64::from(identity.process_group_id),
						i64::from(identity.session_id),
						revision,
						now,
						generation_id.as_str()
					],
				)
				.map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(ProcessGenerationMutationOutcome::Applied(ProcessGenerationMutation {
				revision,
				state: ProcessGenerationState::Starting,
				recorded_at_micros: now,
			}))
		})
		.await
	}

	pub async fn mark_process_generation_ready(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		self.transition_generation(
			generation_id,
			expected_revision,
			ProcessGenerationState::Starting,
			ProcessGenerationState::Ready,
			None,
		)
		.await
	}

	pub async fn mark_process_generation_stopping(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		self.transition_generation(
			generation_id,
			expected_revision,
			ProcessGenerationState::Ready,
			ProcessGenerationState::Stopping,
			None,
		)
		.await
	}

	pub async fn mark_process_generation_death_unknown(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		reason: ProcessAuthorityLossReason,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		let generation_id = generation_id.clone();
		self.mutate_generation(generation_id, expected_revision, move |connection, current, now| {
			if current.state == ProcessGenerationState::DeathUnknown
				&& current.authority_loss_reason == Some(reason)
				&& current.revision == expected_revision.saturating_add(1)
			{
				return Ok(ProcessGenerationMutationOutcome::Replayed(mutation(current)));
			}
			if current.revision != expected_revision
				|| current.state == ProcessGenerationState::Dead
			{
				return Ok(rejected(ProcessGenerationRejection::StaleGeneration, current));
			}
			let revision = expected_revision + 1;
			connection.execute(
				"UPDATE process_generations SET state = 'death_unknown', authority_loss_reason = ?1,
				 revision = ?2, updated_at_micros = ?3 WHERE generation_id = ?4",
				params![reason.as_sql(), revision, now, current.generation_id.as_str()],
			).map_err(sql_error)?;
			Ok(ProcessGenerationMutationOutcome::Applied(ProcessGenerationMutation {
				revision,
				state: ProcessGenerationState::DeathUnknown,
				recorded_at_micros: now,
			}))
		})
		.await
	}

	pub async fn record_process_generation_death(
		&self,
		expected_revision: i64,
		evidence: &ProcessDeathEvidence,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		let evidence = evidence.clone();
		let generation_id = evidence.generation_id.clone();
		self.mutate_generation(generation_id, expected_revision, move |connection, current, now| {
			if current.state == ProcessGenerationState::Dead {
				return if current.death_evidence_id.as_ref() == Some(&evidence.evidence_id) {
					Ok(ProcessGenerationMutationOutcome::Replayed(mutation(current)))
				} else {
					Ok(rejected(ProcessGenerationRejection::EvidenceConflict, current))
				};
			}
			if current.revision != expected_revision
				|| evidence.process_identity.as_ref() != current.process_identity.as_ref()
			{
				return Ok(rejected(ProcessGenerationRejection::EvidenceMismatch, current));
			}
			let (bound_boot, process_id, start_id, group_id, session_id) = evidence
				.process_identity
				.as_ref()
				.map_or((None, None, None, None, None), |identity| {
					(
						Some(identity.boot_id.as_str()),
						Some(i64::from(identity.process_id)),
						Some(identity.process_start_id.as_str()),
						Some(i64::from(identity.process_group_id)),
						Some(i64::from(identity.session_id)),
					)
				});
			connection
				.execute(
					"INSERT INTO process_generation_death_evidence (
				 evidence_id, generation_id, kind, observed_boot_id, bound_boot_id, process_id,
				 process_start_id, process_group_id, session_id, witness_sha256, observed_at_micros
				 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
					params![
						evidence.evidence_id.as_str(),
						evidence.generation_id.as_str(),
						evidence.kind.as_sql(),
						evidence.observed_boot_id.as_str(),
						bound_boot,
						process_id,
						start_id,
						group_id,
						session_id,
						evidence.witness_digest,
						now
					],
				)
				.map_err(sql_error)?;
			let revision = expected_revision + 1;
			connection
				.execute(
					"UPDATE process_generations SET state = 'dead', authority_loss_reason = NULL,
				 death_evidence_id = ?1, revision = ?2, updated_at_micros = ?3
				 WHERE generation_id = ?4",
					params![
						evidence.evidence_id.as_str(),
						revision,
						now,
						current.generation_id.as_str()
					],
				)
				.map_err(sql_error)?;
			Ok(ProcessGenerationMutationOutcome::Applied(ProcessGenerationMutation {
				revision,
				state: ProcessGenerationState::Dead,
				recorded_at_micros: now,
			}))
		})
		.await
	}

	pub async fn project_process_generations_after_supervisor_loss(
		&self,
	) -> Result<u64, StoreError> {
		self.run(move |connection| {
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = connection
				.execute(
					"UPDATE process_generations SET state = 'death_unknown',
				 authority_loss_reason = 'supervisor_restarted', revision = revision + 1,
				 updated_at_micros = ?1 WHERE state NOT IN ('dead', 'death_unknown')",
					params![now],
				)
				.map_err(sql_error)?;
			u64::try_from(changed).map_err(|_| incompatible("generation projection count"))
		})
		.await
	}

	async fn transition_generation(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		expected_state: ProcessGenerationState,
		target_state: ProcessGenerationState,
		_loss: Option<ProcessAuthorityLossReason>,
	) -> Result<ProcessGenerationMutationOutcome, StoreError> {
		if expected_revision <= 0 {
			return Err(StoreError::InvalidInput("ProcessGeneration revision must be positive"));
		}
		let generation_id = generation_id.clone();
		self.mutate_generation(generation_id, expected_revision, move |connection, current, now| {
			if current.state == target_state
				&& current.revision == expected_revision.saturating_add(1)
			{
				return Ok(ProcessGenerationMutationOutcome::Replayed(mutation(current)));
			}
			if current.revision != expected_revision
				|| current.state != expected_state
				|| (target_state == ProcessGenerationState::Ready
					&& current.process_identity.is_none())
			{
				return Ok(rejected(ProcessGenerationRejection::StaleGeneration, current));
			}
			let revision = expected_revision + 1;
			connection.execute(
				"UPDATE process_generations SET state = ?1, revision = ?2, updated_at_micros = ?3
				 WHERE generation_id = ?4",
				params![target_state.as_sql(), revision, now, current.generation_id.as_str()],
			).map_err(sql_error)?;
			Ok(ProcessGenerationMutationOutcome::Applied(ProcessGenerationMutation {
				revision,
				state: target_state,
				recorded_at_micros: now,
			}))
		})
		.await
	}

	async fn mutate_generation<F>(
		&self,
		generation_id: ProcessGenerationId,
		_expected_revision: i64,
		operation: F,
	) -> Result<ProcessGenerationMutationOutcome, StoreError>
	where
		F: FnOnce(
				&rusqlite::Transaction<'_>,
				&ProcessGeneration,
				i64,
			) -> Result<ProcessGenerationMutationOutcome, StoreError>
			+ Send
			+ 'static,
	{
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let Some(current) = read_generation(&transaction, generation_id.as_str())? else {
				return Ok(ProcessGenerationMutationOutcome::Rejected {
					rejection: ProcessGenerationRejection::GenerationMissing,
					actual: empty_mutation(),
				});
			};
			let now = unix_micros().map_err(StoreError::from)?;
			let outcome = operation(&transaction, &current, now)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		})
		.await
	}
}

fn account_authority(
	connection: &rusqlite::Connection,
	account_id: &AccountId,
	binding: &ProcessGenerationAccountBinding,
) -> Result<Option<ProcessGenerationRejection>, StoreError> {
	let row = connection
		.query_row(
			"SELECT a.revision, a.enabled, a.tombstoned_at_micros IS NULL,
		        a.credential_store_observation, c.schema_version, c.credential_version,
		        c.fingerprint, c.writer_operation_id, c.provider, c.provider_account_id,
		        EXISTS (SELECT 1 FROM account_operations AS o WHERE o.account_id = a.account_id
		                AND o.phase NOT IN ('committed', 'cancelled')),
		        EXISTS (SELECT 1 FROM codex_account_capability WHERE singleton = 1
		                AND login_chatgpt_auth_tokens = 1 AND refresh_callback = 1
		                AND callback_profile_sha256 = ?2)
		 FROM accounts AS a LEFT JOIN account_credentials AS c USING (account_id)
		 WHERE a.account_id = ?1",
			params![account_id.as_str(), binding.refresh_callback_profile_sha256],
			|row| {
				Ok((
					row.get::<_, i64>(0)?,
					row.get::<_, bool>(1)?,
					row.get::<_, bool>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, Option<i64>>(4)?,
					row.get::<_, Option<i64>>(5)?,
					row.get::<_, Option<String>>(6)?,
					row.get::<_, Option<String>>(7)?,
					row.get::<_, Option<String>>(8)?,
					row.get::<_, Option<String>>(9)?,
					row.get::<_, bool>(10)?,
					row.get::<_, bool>(11)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?;
	let Some(row) = row else {
		return Ok(Some(ProcessGenerationRejection::AccountMissing));
	};
	if row.0 != binding.account_revision || !row.1 || !row.2 || row.3 != "exact" || row.10 {
		return Ok(Some(ProcessGenerationRejection::AccountLifecycleUnready));
	}
	if !row.11 {
		return Ok(Some(ProcessGenerationRejection::CallbackCapabilityUnready));
	}
	let credential_version = i64::try_from(binding.credential.version.get())
		.map_err(|_| StoreError::InvalidInput("credential version overflows SQLite integer"))?;
	let exact = row.4 == Some(i64::from(binding.credential.schema_version.get()))
		&& row.5 == Some(credential_version)
		&& row.6.as_deref() == Some(binding.credential.fingerprint.as_str())
		&& row.7.as_deref() == Some(binding.credential.writer_operation_id.as_str())
		&& row.8.as_deref() == Some(provider_text(binding.credential.provider.provider()))
		&& row.9.as_deref() == Some(binding.credential.provider.account_id());
	Ok((!exact).then_some(ProcessGenerationRejection::AccountLifecycleUnready))
}

fn read_bound_page(
	connection: &rusqlite::Connection,
	account_id: Option<&str>,
	include_dead: bool,
	after: Option<&str>,
	limit: u16,
) -> Result<Vec<BoundProcessGeneration>, StoreError> {
	let mut statement = connection
		.prepare(
			"SELECT generation_id FROM process_generations
		 WHERE (?1 IS NULL OR account_id = ?1) AND (?2 OR state <> 'dead')
		   AND (?3 IS NULL OR generation_id > ?3)
		 ORDER BY generation_id LIMIT ?4",
		)
		.map_err(sql_error)?;
	let ids = statement
		.query_map(params![account_id, include_dead, after, i64::from(limit)], |row| {
			row.get::<_, String>(0)
		})
		.map_err(sql_error)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(sql_error)?;
	ids.into_iter()
		.map(|id| {
			let generation =
				read_generation(connection, &id)?.ok_or_else(|| incompatible("generation"))?;
			let binding = read_generation_binding(connection, &id)?;
			Ok(BoundProcessGeneration { generation, account_binding: Some(binding) })
		})
		.collect()
}

fn read_generation(
	connection: &rusqlite::Connection,
	id: &str,
) -> Result<Option<ProcessGeneration>, StoreError> {
	connection
		.query_row(
			"SELECT generation_id, account_id, execution_epoch_id, runner_identity,
		        intended_boot_id, control_kind, isolation_kind, bound_boot_id, process_id,
		        process_start_id, process_group_id, session_id, state, revision,
		        authority_loss_reason, death_evidence_id, created_at_micros, updated_at_micros
		 FROM process_generations WHERE generation_id = ?1",
			params![id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, String>(6)?,
					row.get::<_, Option<String>>(7)?,
					row.get::<_, Option<i64>>(8)?,
					row.get::<_, Option<String>>(9)?,
					row.get::<_, Option<i64>>(10)?,
					row.get::<_, Option<i64>>(11)?,
					row.get::<_, String>(12)?,
					row.get::<_, i64>(13)?,
					row.get::<_, Option<String>>(14)?,
					row.get::<_, Option<String>>(15)?,
					row.get::<_, i64>(16)?,
					row.get::<_, i64>(17)?,
				))
			},
		)
		.optional()
		.map_err(sql_error)?
		.map(parse_generation_row)
		.transpose()
}

type GenerationRow = (
	String,
	String,
	String,
	String,
	String,
	String,
	String,
	Option<String>,
	Option<i64>,
	Option<String>,
	Option<i64>,
	Option<i64>,
	String,
	i64,
	Option<String>,
	Option<String>,
	i64,
	i64,
);

fn parse_generation_row(row: GenerationRow) -> Result<ProcessGeneration, StoreError> {
	let intended_boot_id =
		ProcessBootIdentity::new(row.4).map_err(|_| incompatible("boot identity"))?;
	let process_identity = match (row.7, row.8, row.9, row.10, row.11) {
		(None, None, None, None, None) => None,
		(Some(boot), Some(pid), Some(start), Some(group), Some(session)) => Some(
			ProcessIdentity::new(
				ProcessBootIdentity::new(boot).map_err(|_| incompatible("bound boot"))?,
				u32::try_from(pid).map_err(|_| incompatible("process id"))?,
				ProcessStartIdentity::new(start).map_err(|_| incompatible("process start"))?,
				u32::try_from(group).map_err(|_| incompatible("process group"))?,
				u32::try_from(session).map_err(|_| incompatible("session id"))?,
			)
			.map_err(|_| incompatible("process identity"))?,
		),
		_ => return Err(incompatible("partial process identity")),
	};
	Ok(ProcessGeneration {
		generation_id: ProcessGenerationId::new(row.0)
			.map_err(|_| incompatible("generation id"))?,
		account_id: AccountId::new(row.1).map_err(|_| incompatible("account id"))?,
		execution_epoch_id: ProcessExecutionEpochId::new(row.2)
			.map_err(|_| incompatible("epoch id"))?,
		runner_identity: ProcessRunnerIdentity::new(row.3).map_err(|_| incompatible("runner"))?,
		intended_boot_id,
		control_kind: match row.5.as_str() {
			"stdio_only_best_effort_eof" => ProcessControlKind::StdioOnlyBestEffortEof,
			"parent_death_signal_and_stdio_eof" => ProcessControlKind::ParentDeathSignalAndStdioEof,
			_ => return Err(incompatible("control kind")),
		},
		isolation_kind: match row.6.as_str() {
			"session" => ProcessIsolationKind::Session,
			_ => return Err(incompatible("isolation kind")),
		},
		process_identity,
		state: parse_state(&row.12)?,
		authority_loss_reason: row.14.as_deref().map(parse_loss).transpose()?,
		death_evidence_id: row
			.15
			.map(ProcessDeathEvidenceId::new)
			.transpose()
			.map_err(|_| incompatible("death evidence"))?,
		revision: row.13,
		created_at_micros: row.16,
		updated_at_micros: row.17,
	})
}

fn read_generation_binding(
	connection: &rusqlite::Connection,
	id: &str,
) -> Result<ProcessGenerationAccountBinding, StoreError> {
	let row = connection
		.query_row(
			"SELECT account_revision, credential_schema_version, credential_version,
		        credential_fingerprint, credential_writer_operation_id, provider,
		        provider_account_id, refresh_callback_profile_sha256
		 FROM process_generations WHERE generation_id = ?1",
			params![id],
			|row| {
				Ok((
					row.get::<_, i64>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, i64>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, String>(6)?,
					row.get::<_, String>(7)?,
				))
			},
		)
		.map_err(sql_error)?;
	if row.5 != "chatgpt" {
		return Err(incompatible("credential provider"));
	}
	let credential = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::new(
			u16::try_from(row.1).map_err(|_| incompatible("credential schema"))?,
		)
		.map_err(|_| incompatible("credential schema"))?,
		version: CredentialVersion::new(
			u64::try_from(row.2).map_err(|_| incompatible("credential version"))?,
		)
		.map_err(|_| incompatible("credential version"))?,
		fingerprint: CredentialFingerprint::new(row.3)
			.map_err(|_| incompatible("credential fingerprint"))?,
		writer_operation_id: AccountOperationId::new(row.4)
			.map_err(|_| incompatible("credential writer"))?,
		provider: ProviderIdentity::new(AccountProvider::Chatgpt, row.6)
			.map_err(|_| incompatible("provider identity"))?,
	};
	ProcessGenerationAccountBinding::new(row.0, credential, row.7)
		.map_err(|_| incompatible("generation account binding"))
}

fn read_required_generation(
	connection: &rusqlite::Connection,
	id: &ProcessGenerationId,
) -> Result<ProcessGeneration, StoreError> {
	read_generation(connection, id.as_str())?.ok_or_else(|| incompatible("generation missing"))
}

fn parse_state(value: &str) -> Result<ProcessGenerationState, StoreError> {
	match value {
		"starting" => Ok(ProcessGenerationState::Starting),
		"ready" => Ok(ProcessGenerationState::Ready),
		"stopping" => Ok(ProcessGenerationState::Stopping),
		"dead" => Ok(ProcessGenerationState::Dead),
		"death_unknown" => Ok(ProcessGenerationState::DeathUnknown),
		_ => Err(incompatible("generation state")),
	}
}

fn parse_loss(value: &str) -> Result<ProcessAuthorityLossReason, StoreError> {
	match value {
		"supervisor_restarted" => Ok(ProcessAuthorityLossReason::SupervisorRestarted),
		"identity_persistence_failed" => Ok(ProcessAuthorityLossReason::IdentityPersistenceFailed),
		"readiness_persistence_failed" =>
			Ok(ProcessAuthorityLossReason::ReadinessPersistenceFailed),
		"termination_unproved" => Ok(ProcessAuthorityLossReason::TerminationUnproved),
		"control_authority_lost" => Ok(ProcessAuthorityLossReason::ControlAuthorityLost),
		_ => Err(incompatible("authority-loss reason")),
	}
}

fn mutation(generation: &ProcessGeneration) -> ProcessGenerationMutation {
	ProcessGenerationMutation {
		revision: generation.revision,
		state: generation.state,
		recorded_at_micros: generation.updated_at_micros,
	}
}

fn rejected(
	rejection: ProcessGenerationRejection,
	generation: &ProcessGeneration,
) -> ProcessGenerationMutationOutcome {
	ProcessGenerationMutationOutcome::Rejected { rejection, actual: mutation(generation) }
}

fn empty_mutation() -> ProcessGenerationMutation {
	ProcessGenerationMutation {
		revision: 0,
		state: ProcessGenerationState::Starting,
		recorded_at_micros: 0,
	}
}

fn validate_limit(limit: u16) -> Result<(), StoreError> {
	if !(1..=256).contains(&limit) {
		return Err(StoreError::InvalidInput(
			"ProcessGeneration diagnostic limit must be between 1 and 256",
		));
	}
	Ok(())
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

fn incompatible(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
