//! Durable managed-repository authority and the post-COMMIT dispatch boundary.
//!
//! Every mutation reloads PostgreSQL current rows in its transaction. Persisted operation views
//! and restart work are readback-only. Only `prepare_*` can return `RepositoryDispatchReceipt`,
//! and only after that method's transaction receives a successful COMMIT acknowledgement.

use std::path::PathBuf;

use serde_json::{Value, json};
use tokio_postgres::{IsolationLevel, Row, Transaction};

use crate::{PostgresStore, StoreError};
use decodex_core::{
	AdmissionDescriptorDigest, AdmittedRepositoryIdentity, AggregateCheckpoint,
	AllocateRepositoryCommand, AllocationAvailabilityFacts, AssignmentResolution,
	BeginCommitCommand, BeginRegistrationCommand, BeginWorktreeReadyCommand, CanonicalCommitIntent,
	CanonicalOperationDescriptor, CanonicalOperationPayload, CommitEvidence, CommitReadbackRequest,
	CommitReconciliation, ExactRepositoryReadbackScope, ExecutorContractVersion,
	ManagedRepositoryFacts, ManagedRepositoryId, ManagedRepositoryPhase, ManagedWorktreeId,
	NoDispatch, OperationDescriptorVersion, OperationView, PersistedAbsolutePath,
	PositiveAllocationEvidence, ProjectId, RegistrationEvidence, RegistrationReadbackRequest,
	RegistrationReconciliation, RegistrationTarget, RepositoryAdmissionDescriptor,
	RepositoryAdmissionDescriptorVersion, RepositoryAdmissionFacts, RepositoryAdmittedGitLayout,
	RepositoryAllocationId, RepositoryAmbiguity, RepositoryAuthorityTip, RepositoryCommitActor,
	RepositoryCommitActorEmail, RepositoryCommitActorName, RepositoryCommitMessage,
	RepositoryContentRevision, RepositoryEvidenceId, RepositoryGitRegistrationRole,
	RepositoryObservationPath, RepositoryObservedObjectType, RepositoryOperationId,
	RepositoryOperationKind, RepositoryOperationResult, RepositoryOperationState,
	RepositoryPathObservation, RepositoryPathRegistrationRole, RepositoryReferenceName,
	RepositoryRegistrationId, WorktreeReadyEvidence, WorktreeReadyPolicy,
	WorktreeReadyReadbackRequest, WorktreeReadyReconciliation, commit_readback_request,
	decide_allocate, decide_begin_commit, decide_begin_registration, decide_begin_worktree_ready,
	decide_commit_readback, decide_registration_readback, decide_worktree_ready_readback,
	registration_readback_request, resolve_operation_assignment, worktree_ready_readback_request,
};

const MAX_RESTART_WORK: i64 = 256;

/// Owns a pooled connection for the full lifetime of an advisory-lock transaction.
///
/// The connection returns to the fast-recycling pool only after an explicitly acknowledged
/// commit or rollback. Every other exit, including callback unwind, detaches the connection so
/// transaction cleanup and advisory-lock release cannot race reuse from the pool.
struct AdvisoryTransactionOwner {
	client: Option<deadpool_postgres::Client>,
}

impl AdvisoryTransactionOwner {
	fn new(client: deadpool_postgres::Client) -> Self {
		Self { client: Some(client) }
	}

	fn client_mut(&mut self) -> &mut deadpool_postgres::Client {
		self.client.as_mut().expect("advisory transaction owner is live")
	}

	fn confirm(mut self) {
		drop(self.client.take());
	}
}

impl Drop for AdvisoryTransactionOwner {
	fn drop(&mut self) {
		if let Some(client) = self.client.take() {
			drop(deadpool_postgres::Client::take(client));
		}
	}
}

/// Result of immutable repository admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryAdmissionOutcome {
	/// This transaction inserted the immutable admission.
	Admitted,
	/// The complete immutable admission already existed exactly.
	ExistingExact,
}

/// One fresh affine capability created only after successful preparation COMMIT acknowledgement.
///
/// It is intentionally non-`Clone`, non-`Copy`, non-serializable, and publicly unconstructible.
pub struct RepositoryDispatchReceipt {
	descriptor: CanonicalOperationDescriptor,
}
impl RepositoryDispatchReceipt {
	/// Inspect the exact operation this affine capability authorizes.
	pub fn descriptor(&self) -> &CanonicalOperationDescriptor {
		&self.descriptor
	}

	/// Consume the affine capability and recover its exact descriptor for the executor boundary.
	pub fn into_descriptor(self) -> CanonicalOperationDescriptor {
		self.descriptor
	}
}

/// Result of preparing a durably fenced repository effect.
#[allow(clippy::large_enum_variant)] // Preserve the stable by-value adapter result contract.
pub enum RepositoryPreparationOutcome {
	/// A new global assignment committed and yielded one fresh affine receipt.
	Prepared {
		/// Persisted readback view; it does not itself authorize execution.
		operation: OperationView,
		/// Fresh same-control-path post-COMMIT capability.
		receipt: RepositoryDispatchReceipt,
	},
	/// The complete descriptor already existed. This result cannot dispatch.
	ExistingExact(OperationView, NoDispatch),
}

/// Result of serializing one affine receipt against every terminal reconciliation path.
#[allow(missing_docs)] // The type-level contract and field names define the private payloads.
pub enum RepositoryDispatchFenceOutcome<T> {
	/// The exact durable fence was current while the receipt was consumed and dispatched.
	Authorized {
		dispatch: T,
		repository: ManagedRepositoryFacts,
		operation: OperationView,
		/// Whether the lock-only transaction acknowledged release. A false value grants no retry.
		release_confirmed: bool,
	},
	/// Reconciliation won serialization first. The supplied receipt was consumed without dispatch.
	Terminal { repository: ManagedRepositoryFacts, operation: OperationView },
}

/// Readback-only restart work. No variant can authorize an external effect.
#[allow(missing_docs)] // Variant names are the operation-specific readback contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryReadbackWork {
	Registration(RegistrationReadbackRequest),
	WorktreeReady(WorktreeReadyReadbackRequest),
	Commit(CommitReadbackRequest),
}

/// Operation-specific evidence produced while holding the shared dispatch/reconciliation lock.
#[allow(missing_docs)] // Variant names are the operation-specific evidence contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryReadbackEvidence {
	Registration(RegistrationEvidence),
	WorktreeReady(WorktreeReadyEvidence),
	Commit(CommitEvidence),
}

/// Coherent read-only restart state for one committed possibly-effected operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryRestartState {
	/// PostgreSQL current projection loaded in the same read-only snapshot.
	pub repository: ManagedRepositoryFacts,
	/// Complete immutable global assignment and current persisted state.
	pub operation: OperationView,
	/// Immutable positive evidence that originally established the allocation.
	pub allocation_evidence: PositiveAllocationEvidence,
	/// Operation-specific readback work; never an execution permit.
	pub readback: RepositoryReadbackWork,
}

/// Result of operation-specific reconciliation.
#[allow(clippy::large_enum_variant, missing_docs)] // Preserve stable by-value authority facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryReconciliationOutcome {
	/// Readback was temporarily unavailable; durable authority remains possibly effected.
	Pending(OperationView),
	/// One terminal evidence/result/transition transaction committed.
	Terminal { operation: OperationView, repository: ManagedRepositoryFacts },
}

impl PostgresStore {
	/// Whether PostgreSQL currently owns any admitted managed-repository authority.
	pub async fn has_managed_repository_authority(&self) -> Result<bool, StoreError> {
		let client = self.pool().get().await?;
		Ok(client
			.query_one("SELECT EXISTS (SELECT 1 FROM decodex.repository_admissions)", &[])
			.await?
			.get(0))
	}

	/// Insert one immutable repository admission, or accept only an exact repeat.
	pub async fn admit_repository(
		&self,
		admission: &RepositoryAdmissionFacts,
	) -> Result<RepositoryAdmissionOutcome, StoreError> {
		let descriptor = admission.descriptor();
		let descriptor_document = admission_descriptor_document(descriptor)?;
		let mut owner = AdvisoryTransactionOwner::new(self.pool().get().await?);
		let transaction = owner.client_mut().transaction().await?;
		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock(1349,pg_catalog.hashtext($1))",
				&[&descriptor.repository_id().as_str()],
			)
			.await?;
		let inserted = transaction
			.execute(
				"INSERT INTO decodex.repository_admissions(
				 repository_id,project_id,admitted_identity,admitted_base,
				 admission_descriptor_schema,admission_descriptor_digest,
				 admission_descriptor,repository_absolute_path
				) VALUES($1::text::uuid,$2::text::uuid,$3,$4,1,$5,$6,$7)
				 ON CONFLICT DO NOTHING",
				&[
					&descriptor.repository_id().as_str(),
					&descriptor.project_id().as_str(),
					&descriptor.admitted_identity().as_str(),
					&descriptor.admitted_base().as_str(),
					&descriptor.digest().as_str(),
					&descriptor_document,
					&path_text(descriptor.repository_path())?,
				],
			)
			.await?;
		let stored = load_admission(&transaction, descriptor.repository_id()).await?;
		if stored.as_ref() != Some(admission) {
			return Err(StoreError::ManagedRepositoryAdmissionConflict);
		}
		transaction.commit().await?;
		owner.confirm();
		Ok(if inserted == 1 {
			RepositoryAdmissionOutcome::Admitted
		} else {
			RepositoryAdmissionOutcome::ExistingExact
		})
	}

	/// Claim one exact allocation using positive evidence that was acquired read-only.
	pub async fn allocate_repository(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &AllocateRepositoryCommand,
		evidence: &PositiveAllocationEvidence,
	) -> Result<ManagedRepositoryFacts, StoreError> {
		let mut owner = AdvisoryTransactionOwner::new(self.pool().get().await?);
		let transaction = owner.client_mut().transaction().await?;
		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock(1349,pg_catalog.hashtext($1))",
				&[&repository_id.as_str()],
			)
			.await?;
		let admission = load_admission(&transaction, repository_id)
			.await?
			.ok_or(StoreError::InvalidInput("managed repository admission is absent"))?;
		if load_facts(&transaction, repository_id, true).await?.is_some() {
			return Err(StoreError::ManagedRepositoryAlreadyAllocated);
		}
		let unavailable: bool = transaction
			.query_one(
				"SELECT EXISTS(SELECT 1 FROM decodex.managed_repositories
				 WHERE allocation_id=$1::text::uuid OR worktree_id=$2::text::uuid
				 OR worktree_absolute_path=$3)",
				&[
					&command.allocation_id.as_str(),
					&command.worktree_id.as_str(),
					&path_text(&command.worktree_path)?,
				],
			)
			.await?
			.get(0);
		if unavailable {
			return Err(StoreError::ManagedRepositoryAllocationConflict);
		}
		let availability = AllocationAvailabilityFacts {
			allocation_id: command.allocation_id.clone(),
			worktree_id: command.worktree_id.clone(),
			worktree_path: command.worktree_path.clone(),
		};
		let decision = decide_allocate(&admission, &availability, command, evidence)?;
		let tip = issue_authority_tip(&transaction).await?;
		let evidence_document = allocation_evidence_document(evidence)?;
		transaction
			.execute(
				"INSERT INTO decodex.repository_operation_evidence(
				 evidence_id,repository_id,kind,evidence
				) VALUES($1::text::uuid,$2::text::uuid,'allocation',$3)",
				&[&evidence.evidence_id().as_str(), &repository_id.as_str(), &evidence_document],
			)
			.await?;
		transaction
			.execute(
				"INSERT INTO decodex.repository_authority_transitions(
				 repository_id,generation,authority_tip,transition_kind,evidence_id,phase,head
				) VALUES($1::text::uuid,1,$2::text::uuid,'allocated',$3::text::uuid,
				 'allocated',$4)",
				&[
					&repository_id.as_str(),
					&tip.as_str(),
					&evidence.evidence_id().as_str(),
					&decision.head.as_str(),
				],
			)
			.await?;
		transaction
			.execute(
				"INSERT INTO decodex.managed_repositories(
				 repository_id,project_id,allocation_id,worktree_id,worktree_absolute_path,
				 phase,head,generation,authority_tip
				) VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,$4::text::uuid,$5,
				 'allocated',$6,1,$7::text::uuid)",
				&[
					&repository_id.as_str(),
					&decision.admission.descriptor().project_id().as_str(),
					&decision.allocation_id.as_str(),
					&decision.worktree_id.as_str(),
					&path_text(&decision.worktree_path)?,
					&decision.head.as_str(),
					&tip.as_str(),
				],
			)
			.await?;
		let facts = load_facts(&transaction, repository_id, false)
			.await?
			.ok_or_else(|| incompatible("allocated repository projection disappeared"))?;
		transaction.commit().await?;
		owner.confirm();
		Ok(facts)
	}

	/// Load the complete current PostgreSQL projection as readback-only facts.
	pub async fn read_managed_repository(
		&self,
		repository_id: &ManagedRepositoryId,
	) -> Result<Option<ManagedRepositoryFacts>, StoreError> {
		let client = self.pool().get().await?;
		load_facts_client(&client, repository_id).await
	}

	/// Load one globally assigned operation, including its terminal result when present.
	pub async fn read_repository_operation(
		&self,
		operation_id: &RepositoryOperationId,
	) -> Result<Option<OperationView>, StoreError> {
		let client = self.pool().get().await?;
		load_operation_client(&client, operation_id).await
	}

	/// Consume one fresh receipt while holding the operation lock shared by reconciliation.
	///
	/// The callback is synchronous so it cannot outlive the lock-only transaction. PostgreSQL
	/// current rows are reloaded and the exact fence is confirmed under that lock immediately
	/// before the callback receives the affine receipt. If terminal reconciliation acquired the
	/// lock first, the receipt is consumed without calling the callback.
	pub async fn consume_repository_dispatch<T>(
		&self,
		receipt: RepositoryDispatchReceipt,
		dispatch: impl FnOnce(RepositoryDispatchReceipt, &ManagedRepositoryFacts) -> T,
	) -> Result<RepositoryDispatchFenceOutcome<T>, StoreError> {
		let operation_id = receipt.descriptor.operation_id.clone();
		let mut owner = AdvisoryTransactionOwner::new(self.pool().get().await?);
		let transaction = owner.client_mut().transaction().await?;
		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock(1349,pg_catalog.hashtext($1))",
				&[&operation_id.as_str()],
			)
			.await?;
		let operation = load_operation(&transaction, &operation_id)
			.await?
			.ok_or(StoreError::InvalidInput("repository operation is absent"))?;
		if operation.descriptor != receipt.descriptor {
			return Err(StoreError::OperationIdConflict);
		}
		if operation.state != RepositoryOperationState::PossiblyEffected {
			let repository =
				load_facts_client_like(&transaction, &operation.descriptor.repository_id)
					.await?
					.ok_or_else(|| incompatible("terminal operation repository is absent"))?;
			transaction.rollback().await?;
			owner.confirm();
			return Ok(RepositoryDispatchFenceOutcome::Terminal { repository, operation });
		}
		let repository = load_facts(&transaction, &operation.descriptor.repository_id, true)
			.await?
			.ok_or_else(|| incompatible("dispatch repository is absent"))?;
		validate_dispatch_fence(&repository, &operation)?;
		let dispatch = dispatch(receipt, &repository);
		let release_confirmed = match transaction.rollback().await {
			Ok(()) => {
				owner.confirm();
				true
			},
			Err(_) => false,
		};
		Ok(RepositoryDispatchFenceOutcome::Authorized {
			dispatch,
			repository,
			operation,
			release_confirmed,
		})
	}

	/// Prepare a new Register fence or return exact immutable readback with no dispatch.
	pub async fn prepare_registration(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &BeginRegistrationCommand,
	) -> Result<RepositoryPreparationOutcome, StoreError> {
		self.prepare_operation(repository_id, PrepareCommand::Registration(command)).await
	}

	/// Prepare a new WorktreeReady fence or return exact immutable readback with no dispatch.
	pub async fn prepare_worktree_ready(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &BeginWorktreeReadyCommand,
	) -> Result<RepositoryPreparationOutcome, StoreError> {
		self.prepare_operation(repository_id, PrepareCommand::WorktreeReady(command)).await
	}

	/// Prepare a new Commit fence or return exact immutable readback with no dispatch.
	pub async fn prepare_commit(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &BeginCommitCommand,
	) -> Result<RepositoryPreparationOutcome, StoreError> {
		self.prepare_operation(repository_id, PrepareCommand::Commit(command)).await
	}

	async fn prepare_operation(
		&self,
		repository_id: &ManagedRepositoryId,
		command: PrepareCommand<'_>,
	) -> Result<RepositoryPreparationOutcome, StoreError> {
		let mut owner = AdvisoryTransactionOwner::new(self.pool().get().await?);
		let transaction = owner.client_mut().transaction().await?;
		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock(1349,pg_catalog.hashtext($1))",
				&[&command.operation_id().as_str()],
			)
			.await?;
		let existing = load_operation(&transaction, command.operation_id()).await?;
		if existing.as_ref().is_some_and(|view| view.descriptor.repository_id != *repository_id) {
			return Err(StoreError::OperationIdConflict);
		}
		let facts = load_facts(&transaction, repository_id, true)
			.await?
			.ok_or(StoreError::InvalidInput("managed repository is absent"))?;
		let requested = command.descriptor(&facts);
		match resolve_operation_assignment(&requested, existing.as_ref()) {
			AssignmentResolution::ExistingExact(operation, no_dispatch) => {
				transaction.commit().await?;
				owner.confirm();
				return Ok(RepositoryPreparationOutcome::ExistingExact(operation, no_dispatch));
			},
			AssignmentResolution::OperationIdConflict => {
				return Err(StoreError::OperationIdConflict);
			},
			AssignmentResolution::NewlyAssigned => {},
		}
		let operation = command.decide(&facts)?;
		let descriptor_document = descriptor_document(&operation.descriptor)?;
		let payload_document = descriptor_document_payload(&operation.descriptor)?;
		insert_operation(
			&transaction,
			&operation.descriptor,
			&payload_document,
			&descriptor_document,
		)
		.await?;
		transaction
			.execute(
				"INSERT INTO decodex.repository_operation_events(
				 operation_id,ordinal,repository_id,state
				) VALUES($1::text::uuid,1,$2::text::uuid,'possibly_effected')",
				&[
					&operation.descriptor.operation_id.as_str(),
					&operation.descriptor.repository_id.as_str(),
				],
			)
			.await?;
		let next_tip = issue_authority_tip(&transaction).await?;
		let next_generation = next_generation(facts.checkpoint.generation)?;
		let transition_kind = match operation.descriptor.kind {
			RepositoryOperationKind::Register => "register_prepared",
			RepositoryOperationKind::WorktreeReady => "worktree_ready_prepared",
			RepositoryOperationKind::Commit => "commit_prepared",
		};
		insert_transition(
			&transaction,
			&facts,
			next_generation,
			&next_tip,
			transition_kind,
			Some(&operation.descriptor.operation_id),
			None,
			facts.phase,
			&facts.head,
			Some(&operation.descriptor.operation_id),
		)
		.await?;
		let updated = transaction
			.execute(
				"UPDATE decodex.managed_repositories SET generation=$1,authority_tip=$2::text::uuid,
				 active_operation_id=$3::text::uuid,updated_at=pg_catalog.clock_timestamp()
				 WHERE repository_id=$4::text::uuid AND generation=$5
				 AND authority_tip=$6::text::uuid AND active_operation_id IS NULL",
				&[
					&generation_i64(next_generation)?,
					&next_tip.as_str(),
					&operation.descriptor.operation_id.as_str(),
					&repository_id.as_str(),
					&generation_i64(facts.checkpoint.generation)?,
					&facts.checkpoint.tip.as_str(),
				],
			)
			.await?;
		if updated != 1 {
			return Err(StoreError::ManagedRepositoryCompareAndSwapConflict);
		}
		let receipt_seed = operation.descriptor.clone();
		transaction.commit().await.map_err(StoreError::RepositoryCommitOutcomeUnknown)?;
		owner.confirm();
		Ok(RepositoryPreparationOutcome::Prepared {
			operation,
			receipt: RepositoryDispatchReceipt { descriptor: receipt_seed },
		})
	}

	/// Observe and reconcile one operation while holding the lock shared with affine dispatch.
	pub async fn reconcile_repository_readback(
		&self,
		operation_id: &RepositoryOperationId,
		observe: impl FnOnce(
			&RepositoryReadbackWork,
			&RepositoryAdmissionFacts,
			RepositoryEvidenceId,
		) -> RepositoryReadbackEvidence,
	) -> Result<RepositoryReconciliationOutcome, StoreError> {
		let mut owner = AdvisoryTransactionOwner::new(self.pool().get().await?);
		let transaction = owner.client_mut().transaction().await?;
		transaction
			.query_one(
				"SELECT pg_catalog.pg_advisory_xact_lock(1349,pg_catalog.hashtext($1))",
				&[&operation_id.as_str()],
			)
			.await?;
		let operation = load_operation(&transaction, operation_id)
			.await?
			.ok_or(StoreError::InvalidInput("repository operation is absent"))?;
		if operation.state != RepositoryOperationState::PossiblyEffected {
			let repository =
				load_facts_client_like(&transaction, &operation.descriptor.repository_id)
					.await?
					.ok_or_else(|| incompatible("terminal operation repository is absent"))?;
			transaction.commit().await?;
			owner.confirm();
			return Ok(RepositoryReconciliationOutcome::Terminal { repository, operation });
		}
		let facts = load_facts(&transaction, &operation.descriptor.repository_id, true)
			.await?
			.ok_or_else(|| incompatible("operation repository is absent"))?;
		let work = operation_readback_work(&operation)?;
		let value: String =
			transaction.query_one("SELECT pg_catalog.gen_random_uuid()::text", &[]).await?.get(0);
		let evidence_id = RepositoryEvidenceId::new(value)?;
		let observed = observe(&work, &facts.admission, evidence_id);
		let outcome = match observed {
			RepositoryReadbackEvidence::Registration(evidence)
				if matches!(&work, RepositoryReadbackWork::Registration(_)) =>
				finish_reconciliation(
					&transaction,
					operation_id,
					operation,
					facts,
					ReconcileEvidence::Registration(&evidence),
				)
				.await,
			RepositoryReadbackEvidence::WorktreeReady(evidence)
				if matches!(&work, RepositoryReadbackWork::WorktreeReady(_)) =>
				finish_reconciliation(
					&transaction,
					operation_id,
					operation,
					facts,
					ReconcileEvidence::WorktreeReady(&evidence),
				)
				.await,
			RepositoryReadbackEvidence::Commit(evidence)
				if matches!(&work, RepositoryReadbackWork::Commit(_)) =>
				finish_reconciliation(
					&transaction,
					operation_id,
					operation,
					facts,
					ReconcileEvidence::Commit(&evidence),
				)
				.await,
			_ => Err(incompatible("readback evidence kind contradicts operation")),
		}?;
		transaction.commit().await?;
		owner.confirm();
		Ok(outcome)
	}

	/// Load bounded restart work for committed possibly-effected operations only.
	pub async fn load_repository_restart_work(
		&self,
		limit: i64,
	) -> Result<Vec<RepositoryRestartState>, StoreError> {
		if !(1..=MAX_RESTART_WORK).contains(&limit) {
			return Err(StoreError::InvalidInput("repository restart-work limit must be 1..=256"));
		}
		let mut client = self.pool().get().await?;
		let transaction = client
			.build_transaction()
			.isolation_level(IsolationLevel::RepeatableRead)
			.read_only(true)
			.start()
			.await?;
		let rows = transaction
			.query(
				"SELECT operation.descriptor,result.state::text,result.ambiguity::text,result.result,
				 admission.project_id::text,admission.repository_id::text,
				 admission.admitted_identity,admission.admitted_base,
				 admission.admission_descriptor_digest,admission.repository_absolute_path,
				 admission.admission_descriptor_schema,admission.admission_descriptor,
				 repository.repository_id::text,evidence.evidence
				 FROM decodex.managed_repositories repository
				 JOIN decodex.repository_operations operation
				 ON operation.operation_id=repository.active_operation_id
				 AND operation.repository_id=repository.repository_id
				 JOIN decodex.repository_admissions admission
				 ON admission.repository_id=repository.repository_id
				 LEFT JOIN decodex.repository_operation_results result
				 ON result.operation_id=operation.operation_id
				 AND result.repository_id=operation.repository_id
				 JOIN decodex.repository_operation_evidence evidence
				 ON evidence.repository_id=repository.repository_id
				 AND evidence.operation_id IS NULL AND evidence.kind='allocation'
				 ORDER BY operation.assigned_at,operation.operation_id LIMIT $1",
				&[&limit],
			)
			.await?;
		let mut states = Vec::with_capacity(rows.len());
		for row in rows {
			let operation = parse_operation_columns(&row)?;
			let repository_id = ManagedRepositoryId::new(row.get::<_, String>(12))?;
			let repository = load_facts(&transaction, &repository_id, false)
				.await?
				.ok_or_else(|| incompatible("restart repository projection disappeared"))?;
			let allocation_evidence = parse_allocation_evidence(row.get(13))?;
			if allocation_evidence.admission_descriptor() != repository.admission.descriptor()
				|| allocation_evidence.vacant_worktree_path() != &repository.worktree_path
			{
				return Err(incompatible(
					"stored allocation evidence contradicts the current repository authority",
				));
			}
			let readback = match operation.descriptor.kind {
				RepositoryOperationKind::Register => registration_readback_request(&operation)
					.map(RepositoryReadbackWork::Registration),
				RepositoryOperationKind::WorktreeReady =>
					worktree_ready_readback_request(&operation)
						.map(RepositoryReadbackWork::WorktreeReady),
				RepositoryOperationKind::Commit =>
					commit_readback_request(&operation).map(RepositoryReadbackWork::Commit),
			}
			.map_err(StoreError::from)?;
			states.push(RepositoryRestartState {
				repository,
				operation,
				allocation_evidence,
				readback,
			});
		}
		transaction.commit().await?;
		Ok(states)
	}
}

fn operation_readback_work(
	operation: &OperationView,
) -> Result<RepositoryReadbackWork, StoreError> {
	Ok(match operation.descriptor.kind {
		RepositoryOperationKind::Register =>
			RepositoryReadbackWork::Registration(registration_readback_request(operation)?),
		RepositoryOperationKind::WorktreeReady =>
			RepositoryReadbackWork::WorktreeReady(worktree_ready_readback_request(operation)?),
		RepositoryOperationKind::Commit =>
			RepositoryReadbackWork::Commit(commit_readback_request(operation)?),
	})
}

async fn finish_reconciliation(
	transaction: &Transaction<'_>,
	operation_id: &RepositoryOperationId,
	operation: OperationView,
	facts: ManagedRepositoryFacts,
	evidence: ReconcileEvidence<'_>,
) -> Result<RepositoryReconciliationOutcome, StoreError> {
	let terminal = evidence.decide(&facts, &operation)?;
	let Some(terminal) = terminal else {
		return Ok(RepositoryReconciliationOutcome::Pending(operation));
	};
	let evidence_id = terminal.evidence_id(transaction).await?;
	transaction
		.execute(
			"INSERT INTO decodex.repository_operation_evidence(
			 evidence_id,repository_id,operation_id,kind,evidence
			) VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,
			 $4::text::decodex.repository_evidence_kind,$5)",
			&[
				&evidence_id.as_str(),
				&operation.descriptor.repository_id.as_str(),
				&operation_id.as_str(),
				&terminal.evidence_kind(),
				&terminal.evidence_document()?,
			],
		)
		.await?;
	let (state, ambiguity, result) = operation_result_parts(&terminal.operation)?;
	transaction
		.execute(
			"INSERT INTO decodex.repository_operation_results(
			 operation_id,repository_id,state,ambiguity,result,evidence_id
			) VALUES($1::text::uuid,$2::text::uuid,
			 $3::text::decodex.repository_operation_state,
			 $4::text::decodex.repository_ambiguity,$5,$6::text::uuid)",
			&[
				&operation_id.as_str(),
				&operation.descriptor.repository_id.as_str(),
				&state,
				&ambiguity,
				&result,
				&evidence_id.as_str(),
			],
		)
		.await?;
	transaction
		.execute(
			"INSERT INTO decodex.repository_operation_events(
			 operation_id,ordinal,repository_id,state,evidence_id
			) VALUES($1::text::uuid,2,$2::text::uuid,
			 $3::text::decodex.repository_operation_state,$4::text::uuid)",
			&[
				&operation_id.as_str(),
				&operation.descriptor.repository_id.as_str(),
				&state,
				&evidence_id.as_str(),
			],
		)
		.await?;
	let next_tip = issue_authority_tip(transaction).await?;
	let next_generation = next_generation(facts.checkpoint.generation)?;
	insert_transition(
		transaction,
		&facts,
		next_generation,
		&next_tip,
		terminal.transition_kind(),
		Some(operation_id),
		Some(&evidence_id),
		terminal.phase,
		&terminal.head,
		None,
	)
	.await?;
	let updated = transaction
		.execute(
			"UPDATE decodex.managed_repositories SET phase=$1::text::decodex.managed_repository_phase,
			 ambiguity=$2::text::decodex.repository_ambiguity,head=$3,generation=$4,
			 authority_tip=$5::text::uuid,active_operation_id=NULL,
			 updated_at=pg_catalog.clock_timestamp()
			 WHERE repository_id=$6::text::uuid AND generation=$7
			 AND authority_tip=$8::text::uuid AND active_operation_id=$9::text::uuid",
			&[
				&phase_text(terminal.phase),
				&phase_ambiguity_text(terminal.phase),
				&terminal.head.as_str(),
				&generation_i64(next_generation)?,
				&next_tip.as_str(),
				&operation.descriptor.repository_id.as_str(),
				&generation_i64(facts.checkpoint.generation)?,
				&facts.checkpoint.tip.as_str(),
				&operation_id.as_str(),
			],
		)
		.await?;
	if updated != 1 {
		return Err(StoreError::ManagedRepositoryCompareAndSwapConflict);
	}
	let repository = load_facts(transaction, &operation.descriptor.repository_id, false)
		.await?
		.ok_or_else(|| incompatible("reconciled repository projection disappeared"))?;
	Ok(RepositoryReconciliationOutcome::Terminal { operation: terminal.operation, repository })
}

enum PrepareCommand<'a> {
	Registration(&'a BeginRegistrationCommand),
	WorktreeReady(&'a BeginWorktreeReadyCommand),
	Commit(&'a BeginCommitCommand),
}

fn validate_dispatch_fence(
	repository: &ManagedRepositoryFacts,
	operation: &OperationView,
) -> Result<(), StoreError> {
	let (expected_phase, expected_head) = match &operation.descriptor.payload {
		CanonicalOperationPayload::Register { expected_head, .. } =>
			(ManagedRepositoryPhase::Allocated, expected_head),
		CanonicalOperationPayload::WorktreeReady { expected_head, .. } =>
			(ManagedRepositoryPhase::Registered, expected_head),
		CanonicalOperationPayload::Commit { expected_head, .. } =>
			(ManagedRepositoryPhase::Ready, expected_head),
	};
	let expected_generation = operation
		.descriptor
		.expected_checkpoint
		.generation
		.checked_add(1)
		.ok_or_else(|| incompatible("prepared operation generation overflowed"))?;
	if repository.active_operation.as_ref() != Some(&operation.descriptor.operation_id)
		|| repository.phase != expected_phase
		|| repository.head != *expected_head
		|| repository.checkpoint.generation != expected_generation
		|| repository.checkpoint.tip == operation.descriptor.expected_checkpoint.tip
		|| repository.admission.descriptor().repository_id() != &operation.descriptor.repository_id
		|| repository.admission.descriptor().project_id() != &operation.descriptor.project_id
		|| repository.admission.descriptor().admitted_identity()
			!= &operation.descriptor.admitted_identity
		|| repository.admission.descriptor().admitted_base() != &operation.descriptor.admitted_base
		|| repository.admission.descriptor().digest()
			!= &operation.descriptor.admission_descriptor_digest
		|| repository.admission.descriptor().repository_path()
			!= &operation.descriptor.repository_absolute_path
		|| repository.allocation_id != operation.descriptor.allocation_id
		|| repository.worktree_id != operation.descriptor.worktree_id
		|| repository.worktree_path != operation.descriptor.worktree_absolute_path
	{
		return Err(incompatible("durable dispatch fence contradicts operation assignment"));
	}
	Ok(())
}

impl PrepareCommand<'_> {
	fn operation_id(&self) -> &RepositoryOperationId {
		match self {
			Self::Registration(command) => &command.operation_id,
			Self::WorktreeReady(command) => &command.operation_id,
			Self::Commit(command) => &command.operation_id,
		}
	}

	fn descriptor(&self, facts: &ManagedRepositoryFacts) -> CanonicalOperationDescriptor {
		let admission = facts.admission.descriptor();
		let (operation_id, expected_checkpoint, payload, executor_contract) = match self {
			Self::Registration(command) => (
				command.operation_id.clone(),
				command.expected_checkpoint.clone(),
				CanonicalOperationPayload::Register {
					expected_head: command.expected_head.clone(),
					target: RegistrationTarget {
						repository_id: admission.repository_id().clone(),
						worktree_id: facts.worktree_id.clone(),
						repository_path: admission.repository_path().clone(),
						worktree_path: facts.worktree_path.clone(),
					},
				},
				command.executor_contract,
			),
			Self::WorktreeReady(command) => (
				command.operation_id.clone(),
				command.expected_checkpoint.clone(),
				CanonicalOperationPayload::WorktreeReady {
					expected_head: command.expected_head.clone(),
					policy: command.policy,
				},
				command.executor_contract,
			),
			Self::Commit(command) => (
				command.operation_id.clone(),
				command.expected_checkpoint.clone(),
				CanonicalOperationPayload::Commit {
					expected_head: command.expected_head.clone(),
					next_head: command.next_head.clone(),
					intent: command.intent.clone(),
				},
				command.executor_contract,
			),
		};
		CanonicalOperationDescriptor {
			schema: OperationDescriptorVersion::V1,
			operation_id,
			project_id: admission.project_id().clone(),
			repository_id: admission.repository_id().clone(),
			admitted_identity: admission.admitted_identity().clone(),
			admitted_base: admission.admitted_base().clone(),
			admission_descriptor_digest: admission.digest().clone(),
			allocation_id: facts.allocation_id.clone(),
			worktree_id: facts.worktree_id.clone(),
			repository_absolute_path: admission.repository_path().clone(),
			worktree_absolute_path: facts.worktree_path.clone(),
			expected_checkpoint,
			kind: payload.kind(),
			payload,
			executor_contract,
		}
	}

	fn decide(&self, facts: &ManagedRepositoryFacts) -> Result<OperationView, StoreError> {
		Ok(match self {
			Self::Registration(command) => decide_begin_registration(facts, command)?.operation,
			Self::WorktreeReady(command) => decide_begin_worktree_ready(facts, command)?.operation,
			Self::Commit(command) => decide_begin_commit(facts, command)?.operation,
		})
	}
}

enum ReconcileEvidence<'a> {
	Registration(&'a RegistrationEvidence),
	WorktreeReady(&'a WorktreeReadyEvidence),
	Commit(&'a CommitEvidence),
}
impl ReconcileEvidence<'_> {
	fn decide(
		&self,
		facts: &ManagedRepositoryFacts,
		operation: &OperationView,
	) -> Result<Option<TerminalReconciliation>, StoreError> {
		match self {
			Self::Registration(evidence) =>
				match decide_registration_readback(facts, operation, evidence)? {
					RegistrationReconciliation::Pending => Ok(None),
					RegistrationReconciliation::Completed { operation, repository, evidence }
					| RegistrationReconciliation::Ambiguous { operation, repository, evidence } =>
						Ok(Some(TerminalReconciliation::new(
							operation,
							repository.phase,
							repository.head,
							TerminalEvidence::Registration(evidence),
						))),
				},
			Self::WorktreeReady(evidence) =>
				match decide_worktree_ready_readback(facts, operation, evidence)? {
					WorktreeReadyReconciliation::Pending => Ok(None),
					WorktreeReadyReconciliation::Completed { operation, repository, evidence }
					| WorktreeReadyReconciliation::Ambiguous { operation, repository, evidence } =>
						Ok(Some(TerminalReconciliation::new(
							operation,
							repository.phase,
							repository.head,
							TerminalEvidence::WorktreeReady(evidence),
						))),
				},
			Self::Commit(evidence) => match decide_commit_readback(facts, operation, evidence)? {
				CommitReconciliation::Pending => Ok(None),
				CommitReconciliation::Completed { operation, repository, evidence }
				| CommitReconciliation::Ambiguous { operation, repository, evidence } =>
					Ok(Some(TerminalReconciliation::new(
						operation,
						repository.phase,
						repository.head,
						TerminalEvidence::Commit(evidence),
					))),
			},
		}
	}
}

struct TerminalReconciliation {
	operation: OperationView,
	phase: ManagedRepositoryPhase,
	head: RepositoryContentRevision,
	evidence: TerminalEvidence,
}
impl TerminalReconciliation {
	fn new(
		operation: OperationView,
		phase: ManagedRepositoryPhase,
		head: RepositoryContentRevision,
		evidence: TerminalEvidence,
	) -> Self {
		Self { operation, phase, head, evidence }
	}

	async fn evidence_id(
		&self,
		transaction: &Transaction<'_>,
	) -> Result<RepositoryEvidenceId, StoreError> {
		if let Some(id) = self.evidence.exact_evidence_id() {
			Ok(id.clone())
		} else {
			let value: String = transaction
				.query_one("SELECT pg_catalog.gen_random_uuid()::text", &[])
				.await?
				.get(0);
			RepositoryEvidenceId::new(value).map_err(StoreError::from)
		}
	}

	fn evidence_kind(&self) -> &'static str {
		self.evidence.kind()
	}

	fn evidence_document(&self) -> Result<Value, StoreError> {
		self.evidence.document()
	}

	fn transition_kind(&self) -> &'static str {
		if matches!(self.operation.state, RepositoryOperationState::Ambiguous(_)) {
			"operation_ambiguous"
		} else {
			match self.operation.descriptor.kind {
				RepositoryOperationKind::Register => "register_completed",
				RepositoryOperationKind::WorktreeReady => "worktree_ready_completed",
				RepositoryOperationKind::Commit => "commit_completed",
			}
		}
	}
}

enum TerminalEvidence {
	Registration(RegistrationEvidence),
	WorktreeReady(WorktreeReadyEvidence),
	Commit(CommitEvidence),
}
impl TerminalEvidence {
	fn kind(&self) -> &'static str {
		match self {
			Self::Registration(_) => "registration",
			Self::WorktreeReady(_) => "worktree_ready",
			Self::Commit(_) => "commit",
		}
	}

	fn exact_evidence_id(&self) -> Option<&RepositoryEvidenceId> {
		match self {
			Self::Registration(RegistrationEvidence::ExactReciprocal(value)) =>
				Some(&value.scope.evidence_id),
			Self::WorktreeReady(WorktreeReadyEvidence::Exact(value)) =>
				Some(&value.scope.evidence_id),
			Self::Commit(CommitEvidence::Exact(value)) => Some(&value.scope.evidence_id),
			_ => None,
		}
	}

	fn document(&self) -> Result<Value, StoreError> {
		match self {
			Self::Registration(value) => registration_evidence_document(value),
			Self::WorktreeReady(value) => worktree_ready_evidence_document(value),
			Self::Commit(value) => commit_evidence_document(value),
		}
	}
}

async fn load_admission(
	transaction: &Transaction<'_>,
	repository_id: &ManagedRepositoryId,
) -> Result<Option<RepositoryAdmissionFacts>, StoreError> {
	let row = transaction
		.query_opt(
			"SELECT project_id::text,repository_id::text,admitted_identity,admitted_base,
			 admission_descriptor_digest,repository_absolute_path,
			 admission_descriptor_schema,admission_descriptor
			 FROM decodex.repository_admissions WHERE repository_id=$1::text::uuid",
			&[&repository_id.as_str()],
		)
		.await?;
	row.as_ref().map(parse_admission_row).transpose()
}

fn parse_admission_row(row: &Row) -> Result<RepositoryAdmissionFacts, StoreError> {
	parse_admission_columns(row, 0)
}

fn parse_admission_columns(
	row: &Row,
	start: usize,
) -> Result<RepositoryAdmissionFacts, StoreError> {
	let project_id = ProjectId::new(row.get::<_, String>(start))
		.map_err(|_| incompatible("stored admission Project identity is invalid"))?;
	let repository_id = ManagedRepositoryId::new(row.get::<_, String>(start + 1))
		.map_err(|_| incompatible("stored admission repository identity is invalid"))?;
	let admitted_identity = AdmittedRepositoryIdentity::new(row.get::<_, String>(start + 2))
		.map_err(|_| incompatible("stored admission external identity is invalid"))?;
	let admitted_base = RepositoryContentRevision::new(row.get::<_, String>(start + 3))
		.map_err(|_| incompatible("stored admission base is invalid"))?;
	let digest = AdmissionDescriptorDigest::new(row.get::<_, String>(start + 4))
		.map_err(|_| incompatible("stored admission descriptor digest is invalid"))?;
	let repository_path = parse_stored_path(&row.get::<_, String>(start + 5))?;
	let schema: i16 = row.get(start + 6);
	if schema != 1 {
		return Err(incompatible("stored admission descriptor version is unsupported"));
	}
	let document: Value = row.get(start + 7);
	let descriptor = parse_admission_descriptor_document(&document)?;
	if descriptor.version() != RepositoryAdmissionDescriptorVersion::V1
		|| descriptor.project_id() != &project_id
		|| descriptor.repository_id() != &repository_id
		|| descriptor.admitted_identity() != &admitted_identity
		|| descriptor.admitted_base() != &admitted_base
		|| descriptor.repository_path() != &repository_path
		|| !descriptor.verify_digest(&digest)
	{
		return Err(incompatible("stored admission columns contradict the complete descriptor"));
	}
	Ok(RepositoryAdmissionFacts::new(descriptor))
}

const FACTS_SELECT: &str = "SELECT admission.project_id::text,admission.repository_id::text,
 admission.admitted_identity,admission.admitted_base,admission.admission_descriptor_digest,
 admission.repository_absolute_path,admission.admission_descriptor_schema,
 admission.admission_descriptor,repository.allocation_id::text,repository.worktree_id::text,
	 repository.worktree_absolute_path,repository.phase::text,repository.ambiguity::text,
	 repository.head,repository.generation,repository.authority_tip::text,
	 repository.active_operation_id::text
 FROM decodex.managed_repositories repository JOIN decodex.repository_admissions admission
 USING(repository_id) WHERE repository.repository_id=$1::text::uuid";

async fn load_facts(
	transaction: &Transaction<'_>,
	repository_id: &ManagedRepositoryId,
	for_update: bool,
) -> Result<Option<ManagedRepositoryFacts>, StoreError> {
	let statement = if for_update {
		format!("{FACTS_SELECT} FOR UPDATE OF repository")
	} else {
		FACTS_SELECT.to_owned()
	};
	transaction
		.query_opt(&statement, &[&repository_id.as_str()])
		.await?
		.map(parse_facts_row)
		.transpose()
}

async fn load_facts_client(
	client: &deadpool_postgres::Client,
	repository_id: &ManagedRepositoryId,
) -> Result<Option<ManagedRepositoryFacts>, StoreError> {
	client
		.query_opt(FACTS_SELECT, &[&repository_id.as_str()])
		.await?
		.map(parse_facts_row)
		.transpose()
}

async fn load_facts_client_like(
	transaction: &Transaction<'_>,
	repository_id: &ManagedRepositoryId,
) -> Result<Option<ManagedRepositoryFacts>, StoreError> {
	load_facts(transaction, repository_id, false).await
}

fn parse_facts_row(row: Row) -> Result<ManagedRepositoryFacts, StoreError> {
	let generation: i64 = row.get(14);
	Ok(ManagedRepositoryFacts {
		admission: parse_admission_columns(&row, 0)?,
		allocation_id: RepositoryAllocationId::new(row.get::<_, String>(8))?,
		worktree_id: ManagedWorktreeId::new(row.get::<_, String>(9))?,
		worktree_path: PersistedAbsolutePath::new(PathBuf::from(row.get::<_, String>(10)))?,
		phase: parse_phase(&row.get::<_, String>(11), row.get::<_, Option<String>>(12).as_deref())?,
		head: RepositoryContentRevision::new(row.get::<_, String>(13))?,
		checkpoint: AggregateCheckpoint::new(
			u64::try_from(generation).map_err(|_| incompatible("stored generation is invalid"))?,
			RepositoryAuthorityTip::new(row.get::<_, String>(15))?,
		)?,
		active_operation: row
			.get::<_, Option<String>>(16)
			.map(RepositoryOperationId::new)
			.transpose()?,
	})
}

const OPERATION_SELECT: &str = "SELECT operation.descriptor,result.state::text,
 result.ambiguity::text,result.result,admission.project_id::text,
 admission.repository_id::text,admission.admitted_identity,admission.admitted_base,
 admission.admission_descriptor_digest,admission.repository_absolute_path,
	 admission.admission_descriptor_schema,admission.admission_descriptor
	 FROM decodex.repository_operations operation
	 LEFT JOIN decodex.repository_operation_results result
	 ON result.operation_id=operation.operation_id
	 AND result.repository_id=operation.repository_id
	 JOIN decodex.repository_admissions admission
	 ON admission.repository_id=operation.repository_id
	 WHERE operation.operation_id=$1::text::uuid";

async fn load_operation(
	transaction: &Transaction<'_>,
	operation_id: &RepositoryOperationId,
) -> Result<Option<OperationView>, StoreError> {
	transaction
		.query_opt(OPERATION_SELECT, &[&operation_id.as_str()])
		.await?
		.map(parse_operation_row)
		.transpose()
}

async fn load_operation_client(
	client: &deadpool_postgres::Client,
	operation_id: &RepositoryOperationId,
) -> Result<Option<OperationView>, StoreError> {
	client
		.query_opt(OPERATION_SELECT, &[&operation_id.as_str()])
		.await?
		.map(parse_operation_row)
		.transpose()
}

fn parse_operation_row(row: Row) -> Result<OperationView, StoreError> {
	parse_operation_columns(&row)
}

fn parse_operation_columns(row: &Row) -> Result<OperationView, StoreError> {
	let descriptor = parse_descriptor(row.get(0))?;
	let admission = parse_admission_columns(row, 4)?;
	let admitted = admission.descriptor();
	if &descriptor.project_id != admitted.project_id()
		|| &descriptor.repository_id != admitted.repository_id()
		|| &descriptor.admitted_identity != admitted.admitted_identity()
		|| &descriptor.admitted_base != admitted.admitted_base()
		|| &descriptor.admission_descriptor_digest != admitted.digest()
		|| &descriptor.repository_absolute_path != admitted.repository_path()
	{
		return Err(incompatible(
			"stored operation descriptor contradicts the complete admission authority",
		));
	}
	let state = match row.get::<_, Option<String>>(1).as_deref() {
		None => RepositoryOperationState::PossiblyEffected,
		Some("ambiguous") => RepositoryOperationState::Ambiguous(parse_ambiguity(
			&row.get::<_, Option<String>>(2)
				.ok_or_else(|| incompatible("ambiguous operation result has no reason"))?,
		)?),
		Some("completed") => RepositoryOperationState::Completed(parse_result(
			row.get::<_, Option<Value>>(3)
				.ok_or_else(|| incompatible("completed operation has no result"))?,
		)?),
		Some(_) => return Err(incompatible("stored repository operation state is invalid")),
	};
	Ok(OperationView { descriptor, state })
}

fn parse_allocation_evidence(document: Value) -> Result<PositiveAllocationEvidence, StoreError> {
	if required_str(&document, "classification")? != "positive" {
		return Err(incompatible("stored allocation evidence classification is invalid"));
	}
	let evidence = PositiveAllocationEvidence::new(
		RepositoryEvidenceId::new(required_str(&document, "evidence_id")?)
			.map_err(|_| incompatible("stored allocation evidence identity is invalid"))?,
		parse_admission_descriptor_document(required_value(&document, "admission_descriptor")?)?,
		parse_stored_path(required_str(&document, "vacant_worktree_absolute_path")?)?,
	);
	if allocation_evidence_document(&evidence)? != document {
		return Err(incompatible("stored allocation evidence is not canonical"));
	}
	Ok(evidence)
}

async fn insert_operation(
	transaction: &Transaction<'_>,
	descriptor: &CanonicalOperationDescriptor,
	payload: &Value,
	document: &Value,
) -> Result<(), StoreError> {
	transaction
		.execute(
			"INSERT INTO decodex.repository_operations(
			 operation_id,descriptor_schema,project_id,repository_id,admitted_identity,
			 admitted_base,admission_descriptor_digest,allocation_id,worktree_id,
			 repository_absolute_path,worktree_absolute_path,expected_generation,
			 expected_authority_tip,kind,payload,executor_contract_version,descriptor
			) VALUES($1::text::uuid,1,$2::text::uuid,$3::text::uuid,$4,$5,$6,
			 $7::text::uuid,$8::text::uuid,$9,$10,$11,$12::text::uuid,
			 $13::text::decodex.repository_operation_kind,$14,$15,$16)",
			&[
				&descriptor.operation_id.as_str(),
				&descriptor.project_id.as_str(),
				&descriptor.repository_id.as_str(),
				&descriptor.admitted_identity.as_str(),
				&descriptor.admitted_base.as_str(),
				&descriptor.admission_descriptor_digest.as_str(),
				&descriptor.allocation_id.as_str(),
				&descriptor.worktree_id.as_str(),
				&path_text(&descriptor.repository_absolute_path)?,
				&path_text(&descriptor.worktree_absolute_path)?,
				&generation_i64(descriptor.expected_checkpoint.generation)?,
				&descriptor.expected_checkpoint.tip.as_str(),
				&operation_kind_text(descriptor.kind),
				&payload,
				&i32::from(descriptor.executor_contract.get()),
				&document,
			],
		)
		.await?;
	Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_transition(
	transaction: &Transaction<'_>,
	facts: &ManagedRepositoryFacts,
	generation: u64,
	tip: &RepositoryAuthorityTip,
	kind: &str,
	operation_id: Option<&RepositoryOperationId>,
	evidence_id: Option<&RepositoryEvidenceId>,
	phase: ManagedRepositoryPhase,
	head: &RepositoryContentRevision,
	active_operation_id: Option<&RepositoryOperationId>,
) -> Result<(), StoreError> {
	transaction
		.execute(
			"INSERT INTO decodex.repository_authority_transitions(
			 repository_id,generation,authority_tip,prior_generation,prior_authority_tip,
			 transition_kind,operation_id,evidence_id,phase,ambiguity,head,active_operation_id
			) VALUES($1::text::uuid,$2,$3::text::uuid,$4,$5::text::uuid,
			 $6::text::decodex.repository_authority_transition_kind,$7::text::uuid,$8::text::uuid,
			 $9::text::decodex.managed_repository_phase,
			 $10::text::decodex.repository_ambiguity,$11,$12::text::uuid)",
			&[
				&facts.admission.descriptor().repository_id().as_str(),
				&generation_i64(generation)?,
				&tip.as_str(),
				&generation_i64(facts.checkpoint.generation)?,
				&facts.checkpoint.tip.as_str(),
				&kind,
				&operation_id.map(RepositoryOperationId::as_str),
				&evidence_id.map(RepositoryEvidenceId::as_str),
				&phase_text(phase),
				&phase_ambiguity_text(phase),
				&head.as_str(),
				&active_operation_id.map(RepositoryOperationId::as_str),
			],
		)
		.await?;
	Ok(())
}

async fn issue_authority_tip(
	transaction: &Transaction<'_>,
) -> Result<RepositoryAuthorityTip, StoreError> {
	let value: String =
		transaction.query_one("SELECT pg_catalog.gen_random_uuid()::text", &[]).await?.get(0);
	RepositoryAuthorityTip::new(value).map_err(StoreError::from)
}

fn descriptor_document(descriptor: &CanonicalOperationDescriptor) -> Result<Value, StoreError> {
	Ok(json!({
		"schema": 1,
		"operation_id": descriptor.operation_id.as_str(),
		"project_id": descriptor.project_id.as_str(),
		"repository_id": descriptor.repository_id.as_str(),
		"admitted_identity": descriptor.admitted_identity.as_str(),
		"admitted_base": descriptor.admitted_base.as_str(),
		"admission_descriptor_digest": descriptor.admission_descriptor_digest.as_str(),
		"allocation_id": descriptor.allocation_id.as_str(),
		"worktree_id": descriptor.worktree_id.as_str(),
		"repository_absolute_path": path_text(&descriptor.repository_absolute_path)?,
		"worktree_absolute_path": path_text(&descriptor.worktree_absolute_path)?,
		"expected_generation": descriptor.expected_checkpoint.generation,
		"expected_authority_tip": descriptor.expected_checkpoint.tip.as_str(),
		"kind": operation_kind_text(descriptor.kind),
		"payload": descriptor_document_payload(descriptor)?,
		"executor_contract_version": descriptor.executor_contract.get(),
	}))
}

fn descriptor_document_payload(
	descriptor: &CanonicalOperationDescriptor,
) -> Result<Value, StoreError> {
	Ok(match &descriptor.payload {
		CanonicalOperationPayload::Register { expected_head, target } => json!({
			"kind": "register",
			"expected_head": expected_head.as_str(),
			"target": {
				"repository_id": target.repository_id.as_str(),
				"worktree_id": target.worktree_id.as_str(),
				"repository_absolute_path": path_text(&target.repository_path)?,
				"worktree_absolute_path": path_text(&target.worktree_path)?,
			},
		}),
		CanonicalOperationPayload::WorktreeReady { expected_head, policy } => json!({
			"kind": "worktree_ready",
			"expected_head": expected_head.as_str(),
			"policy": worktree_policy_text(*policy),
		}),
		CanonicalOperationPayload::Commit { expected_head, next_head, intent } => json!({
			"kind": "commit",
			"expected_head": expected_head.as_str(),
			"next_head": next_head.as_str(),
			"intent": commit_intent_document(intent),
		}),
	})
}

fn commit_intent_document(intent: &CanonicalCommitIntent) -> Value {
	json!({
		"target_reference": intent.target_reference.as_str(),
		"tree": intent.tree.as_str(),
		"message": intent.message.as_str(),
		"author": actor_document(&intent.author),
		"committer": actor_document(&intent.committer),
	})
}

fn actor_document(actor: &RepositoryCommitActor) -> Value {
	json!({
		"name": actor.name.as_str(),
		"email": actor.email.as_str(),
		"timestamp_seconds": actor.timestamp_seconds,
		"utc_offset_minutes": actor.utc_offset_minutes,
	})
}

fn admission_descriptor_document(
	descriptor: &RepositoryAdmissionDescriptor,
) -> Result<Value, StoreError> {
	let layout = descriptor.git_layout();
	let observations = descriptor
		.observations()
		.iter()
		.map(|observation| {
			Ok(json!({
				"path": observation_path_text(observation.path())?,
				"roles": observation.roles().iter().copied()
					.map(path_registration_role_text).collect::<Vec<_>>(),
				"device": observation.device(),
				"inode": observation.inode(),
				"object_type": observed_object_type_text(observation.object_type()),
				"owner_uid": observation.owner_uid(),
				"permissions": observation.permissions(),
			}))
		})
		.collect::<Result<Vec<_>, StoreError>>()?;
	Ok(json!({
		"schema": match descriptor.version() {
			RepositoryAdmissionDescriptorVersion::V1 => 1,
		},
		"project_id": descriptor.project_id().as_str(),
		"repository_id": descriptor.repository_id().as_str(),
		"admitted_identity": descriptor.admitted_identity().as_str(),
		"admitted_base": descriptor.admitted_base().as_str(),
		"repository_absolute_path": path_text(descriptor.repository_path())?,
		"git_layout": {
			"registration_role": git_registration_role_text(layout.registration_role()),
			"registration_id": layout.registration_id().map(RepositoryRegistrationId::as_str),
			"repository_absolute_path": path_text(layout.repository_root())?,
			"worktree_git_entry_absolute_path": path_text(layout.worktree_git_entry())?,
			"git_directory_absolute_path": path_text(layout.git_directory())?,
			"common_directory_absolute_path": path_text(layout.common_directory())?,
			"objects_directory_absolute_path": path_text(layout.objects_directory())?,
			"refs_directory_absolute_path": optional_path_text(layout.refs_directory())?,
			"common_directory_file_absolute_path": optional_path_text(
				layout.common_directory_file()
			)?,
			"git_directory_backlink_file_absolute_path": optional_path_text(
				layout.git_directory_backlink_file()
			)?,
		},
		"observations": observations,
		"digest": descriptor.digest().as_str(),
	}))
}

fn parse_admission_descriptor_document(
	document: &Value,
) -> Result<RepositoryAdmissionDescriptor, StoreError> {
	if required_u64(document, "schema")? != 1 {
		return Err(incompatible("stored admission descriptor version is unsupported"));
	}
	let layout_document = required_value(document, "git_layout")?;
	let registration_role =
		parse_git_registration_role(required_str(layout_document, "registration_role")?)?;
	let registration_id = required_optional_str(layout_document, "registration_id")?
		.map(|value| {
			RepositoryRegistrationId::new(value)
				.map_err(|_| incompatible("stored admission registration identity is invalid"))
		})
		.transpose()?;
	let layout = RepositoryAdmittedGitLayout::new(
		registration_role,
		registration_id,
		parse_stored_path(required_str(layout_document, "repository_absolute_path")?)?,
		parse_stored_path(required_str(layout_document, "worktree_git_entry_absolute_path")?)?,
		parse_stored_path(required_str(layout_document, "git_directory_absolute_path")?)?,
		parse_stored_path(required_str(layout_document, "common_directory_absolute_path")?)?,
		parse_stored_path(required_str(layout_document, "objects_directory_absolute_path")?)?,
		parse_optional_stored_path(layout_document, "refs_directory_absolute_path")?,
		parse_optional_stored_path(layout_document, "common_directory_file_absolute_path")?,
		parse_optional_stored_path(layout_document, "git_directory_backlink_file_absolute_path")?,
	);
	let observations_document = required_value(document, "observations")?
		.as_array()
		.ok_or_else(|| incompatible("stored admission observations are not an array"))?;
	let mut observations = Vec::with_capacity(observations_document.len());
	for observation_document in observations_document {
		let roles_document = required_value(observation_document, "roles")?
			.as_array()
			.ok_or_else(|| incompatible("stored admission observation roles are not an array"))?;
		let roles =
			roles_document
				.iter()
				.map(|role| {
					parse_path_registration_role(role.as_str().ok_or_else(|| {
						incompatible("stored admission observation role is invalid")
					})?)
				})
				.collect::<Result<Vec<_>, StoreError>>()?;
		let owner_uid = u32::try_from(required_u64(observation_document, "owner_uid")?)
			.map_err(|_| incompatible("stored admission observation UID is invalid"))?;
		let permissions = u32::try_from(required_u64(observation_document, "permissions")?)
			.map_err(|_| incompatible("stored admission observation mode is invalid"))?;
		observations.push(
			RepositoryPathObservation::new(
				parse_observation_path(required_str(observation_document, "path")?)?,
				roles,
				required_u64(observation_document, "device")?,
				required_u64(observation_document, "inode")?,
				parse_observed_object_type(required_str(observation_document, "object_type")?)?,
				owner_uid,
				permissions,
			)
			.map_err(|_| incompatible("stored admission path observation is invalid"))?,
		);
	}
	let descriptor = RepositoryAdmissionDescriptor::new_v1(
		ProjectId::new(required_str(document, "project_id")?)
			.map_err(|_| incompatible("stored admission Project identity is invalid"))?,
		ManagedRepositoryId::new(required_str(document, "repository_id")?)
			.map_err(|_| incompatible("stored admission repository identity is invalid"))?,
		AdmittedRepositoryIdentity::new(required_str(document, "admitted_identity")?)
			.map_err(|_| incompatible("stored admission external identity is invalid"))?,
		RepositoryContentRevision::new(required_str(document, "admitted_base")?)
			.map_err(|_| incompatible("stored admission base is invalid"))?,
		parse_stored_path(required_str(document, "repository_absolute_path")?)?,
		layout,
		observations,
	)
	.map_err(|_| incompatible("stored admission descriptor is contradictory"))?;
	let persisted_digest = AdmissionDescriptorDigest::new(required_str(document, "digest")?)
		.map_err(|_| incompatible("stored admission descriptor digest is invalid"))?;
	if !descriptor.verify_digest(&persisted_digest)
		|| admission_descriptor_document(&descriptor)? != *document
	{
		return Err(incompatible(
			"stored admission descriptor digest or canonical form is invalid",
		));
	}
	Ok(descriptor)
}

fn allocation_evidence_document(
	evidence: &PositiveAllocationEvidence,
) -> Result<Value, StoreError> {
	Ok(json!({
		"classification": "positive",
		"evidence_id": evidence.evidence_id().as_str(),
		"admission_descriptor": admission_descriptor_document(
			evidence.admission_descriptor()
		)?,
		"vacant_worktree_absolute_path": path_text(evidence.vacant_worktree_path())?,
	}))
}

fn registration_evidence_document(evidence: &RegistrationEvidence) -> Result<Value, StoreError> {
	Ok(match evidence {
		RegistrationEvidence::ExactReciprocal(value) => json!({
			"classification": "exact_reciprocal",
			"scope": scope_document(&value.scope)?,
			"repository_names_worktree": value.repository_names_worktree.as_str(),
			"worktree_names_repository": value.worktree_names_repository.as_str(),
			"unchanged_head": value.unchanged_head.as_str(),
		}),
		other => json!({"classification": registration_evidence_text(other)}),
	})
}

fn worktree_ready_evidence_document(evidence: &WorktreeReadyEvidence) -> Result<Value, StoreError> {
	Ok(match evidence {
		WorktreeReadyEvidence::Exact(value) => json!({
			"classification": "exact",
			"scope": scope_document(&value.scope)?,
			"unchanged_head": value.unchanged_head.as_str(),
		}),
		other => json!({"classification": worktree_ready_evidence_text(other)}),
	})
}

fn commit_evidence_document(evidence: &CommitEvidence) -> Result<Value, StoreError> {
	Ok(match evidence {
		CommitEvidence::Exact(value) => json!({
			"classification": "exact",
			"scope": scope_document(&value.scope)?,
			"target_reference": value.target_reference.as_str(),
			"intent": commit_intent_document(&value.intent),
			"predecessor_head": value.predecessor_head.as_str(),
			"completed_head": value.completed_head.as_str(),
		}),
		other => json!({"classification": commit_evidence_text(other)}),
	})
}

fn scope_document(scope: &ExactRepositoryReadbackScope) -> Result<Value, StoreError> {
	Ok(json!({
		"evidence_id": scope.evidence_id.as_str(),
		"operation_id": scope.operation_id.as_str(),
		"admitted_identity": scope.admitted_identity.as_str(),
		"admitted_base": scope.admitted_base.as_str(),
		"repository_id": scope.repository_id.as_str(),
		"allocation_id": scope.allocation_id.as_str(),
		"worktree_id": scope.worktree_id.as_str(),
		"repository_absolute_path": path_text(&scope.repository_path)?,
		"worktree_absolute_path": path_text(&scope.worktree_path)?,
	}))
}

fn operation_result_parts(
	operation: &OperationView,
) -> Result<(&'static str, Option<&'static str>, Option<Value>), StoreError> {
	match &operation.state {
		RepositoryOperationState::Completed(result) => Ok((
			"completed",
			None,
			Some(match result {
				RepositoryOperationResult::Registered { head } =>
					json!({"kind":"registered","head":head.as_str()}),
				RepositoryOperationResult::WorktreeReady { head } =>
					json!({"kind":"worktree_ready","head":head.as_str()}),
				RepositoryOperationResult::Committed { from, to } =>
					json!({"kind":"committed","from":from.as_str(),"to":to.as_str()}),
			}),
		)),
		RepositoryOperationState::Ambiguous(reason) =>
			Ok(("ambiguous", Some(ambiguity_text(*reason)), None)),
		RepositoryOperationState::PossiblyEffected =>
			Err(incompatible("terminal reconciliation remained possibly effected")),
	}
}

fn parse_descriptor(document: Value) -> Result<CanonicalOperationDescriptor, StoreError> {
	let payload = required_value(&document, "payload")?;
	let kind = parse_operation_kind(required_str(&document, "kind")?)?;
	if required_str(payload, "kind")? != operation_kind_text(kind) {
		return Err(incompatible("stored operation payload kind is inconsistent"));
	}
	let payload = match kind {
		RepositoryOperationKind::Register => CanonicalOperationPayload::Register {
			expected_head: RepositoryContentRevision::new(required_str(payload, "expected_head")?)?,
			target: parse_registration_target(required_value(payload, "target")?)?,
		},
		RepositoryOperationKind::WorktreeReady => CanonicalOperationPayload::WorktreeReady {
			expected_head: RepositoryContentRevision::new(required_str(payload, "expected_head")?)?,
			policy: parse_worktree_policy(required_str(payload, "policy")?)?,
		},
		RepositoryOperationKind::Commit => CanonicalOperationPayload::Commit {
			expected_head: RepositoryContentRevision::new(required_str(payload, "expected_head")?)?,
			next_head: RepositoryContentRevision::new(required_str(payload, "next_head")?)?,
			intent: parse_commit_intent(required_value(payload, "intent")?)?,
		},
	};
	if payload.kind() != kind {
		return Err(incompatible("stored operation payload kind is inconsistent"));
	}
	let schema = match required_u64(&document, "schema")? {
		1 => OperationDescriptorVersion::V1,
		_ => return Err(incompatible("stored operation descriptor schema is unsupported")),
	};
	let executor = u16::try_from(required_u64(&document, "executor_contract_version")?)
		.map_err(|_| incompatible("stored executor contract version is invalid"))?;
	Ok(CanonicalOperationDescriptor {
		schema,
		operation_id: RepositoryOperationId::new(required_str(&document, "operation_id")?)?,
		project_id: ProjectId::new(required_str(&document, "project_id")?)
			.map_err(|_| incompatible("stored Project identity is invalid"))?,
		repository_id: ManagedRepositoryId::new(required_str(&document, "repository_id")?)?,
		admitted_identity: AdmittedRepositoryIdentity::new(required_str(
			&document,
			"admitted_identity",
		)?)?,
		admitted_base: RepositoryContentRevision::new(required_str(&document, "admitted_base")?)?,
		admission_descriptor_digest: AdmissionDescriptorDigest::new(required_str(
			&document,
			"admission_descriptor_digest",
		)?)?,
		allocation_id: RepositoryAllocationId::new(required_str(&document, "allocation_id")?)?,
		worktree_id: ManagedWorktreeId::new(required_str(&document, "worktree_id")?)?,
		repository_absolute_path: parse_path(required_str(&document, "repository_absolute_path")?)?,
		worktree_absolute_path: parse_path(required_str(&document, "worktree_absolute_path")?)?,
		expected_checkpoint: AggregateCheckpoint::new(
			required_u64(&document, "expected_generation")?,
			RepositoryAuthorityTip::new(required_str(&document, "expected_authority_tip")?)?,
		)?,
		kind,
		payload,
		executor_contract: ExecutorContractVersion::new(executor)?,
	})
}

fn parse_registration_target(value: &Value) -> Result<RegistrationTarget, StoreError> {
	Ok(RegistrationTarget {
		repository_id: ManagedRepositoryId::new(required_str(value, "repository_id")?)?,
		worktree_id: ManagedWorktreeId::new(required_str(value, "worktree_id")?)?,
		repository_path: parse_path(required_str(value, "repository_absolute_path")?)?,
		worktree_path: parse_path(required_str(value, "worktree_absolute_path")?)?,
	})
}

fn parse_commit_intent(value: &Value) -> Result<CanonicalCommitIntent, StoreError> {
	Ok(CanonicalCommitIntent {
		target_reference: RepositoryReferenceName::new(required_str(value, "target_reference")?)?,
		tree: RepositoryContentRevision::new(required_str(value, "tree")?)?,
		message: RepositoryCommitMessage::new(required_str(value, "message")?)?,
		author: parse_actor(required_value(value, "author")?)?,
		committer: parse_actor(required_value(value, "committer")?)?,
	})
}

fn parse_actor(value: &Value) -> Result<RepositoryCommitActor, StoreError> {
	RepositoryCommitActor::new(
		RepositoryCommitActorName::new(required_str(value, "name")?)?,
		RepositoryCommitActorEmail::new(required_str(value, "email")?)?,
		required_i64(value, "timestamp_seconds")?,
		i16::try_from(required_i64(value, "utc_offset_minutes")?)
			.map_err(|_| incompatible("stored commit actor UTC offset is invalid"))?,
	)
	.map_err(StoreError::from)
}

fn parse_result(value: Value) -> Result<RepositoryOperationResult, StoreError> {
	match required_str(&value, "kind")? {
		"registered" => Ok(RepositoryOperationResult::Registered {
			head: RepositoryContentRevision::new(required_str(&value, "head")?)?,
		}),
		"worktree_ready" => Ok(RepositoryOperationResult::WorktreeReady {
			head: RepositoryContentRevision::new(required_str(&value, "head")?)?,
		}),
		"committed" => Ok(RepositoryOperationResult::Committed {
			from: RepositoryContentRevision::new(required_str(&value, "from")?)?,
			to: RepositoryContentRevision::new(required_str(&value, "to")?)?,
		}),
		_ => Err(incompatible("stored repository operation result is invalid")),
	}
}

fn parse_phase(value: &str, ambiguity: Option<&str>) -> Result<ManagedRepositoryPhase, StoreError> {
	Ok(match value {
		"allocated" if ambiguity.is_none() => ManagedRepositoryPhase::Allocated,
		"registered" if ambiguity.is_none() => ManagedRepositoryPhase::Registered,
		"ready" if ambiguity.is_none() => ManagedRepositoryPhase::Ready,
		"ambiguous" => ManagedRepositoryPhase::Ambiguous(parse_ambiguity(
			ambiguity.ok_or_else(|| incompatible("ambiguous repository has no reason"))?,
		)?),
		_ => return Err(incompatible("stored managed-repository phase is invalid")),
	})
}

fn parse_operation_kind(value: &str) -> Result<RepositoryOperationKind, StoreError> {
	Ok(match value {
		"register" => RepositoryOperationKind::Register,
		"worktree_ready" => RepositoryOperationKind::WorktreeReady,
		"commit" => RepositoryOperationKind::Commit,
		_ => return Err(incompatible("stored repository operation kind is invalid")),
	})
}

fn parse_worktree_policy(value: &str) -> Result<WorktreeReadyPolicy, StoreError> {
	match value {
		"exact_clean_worktree" => Ok(WorktreeReadyPolicy::ExactCleanWorktree),
		_ => Err(incompatible("stored WorktreeReady policy is invalid")),
	}
}

fn parse_git_registration_role(value: &str) -> Result<RepositoryGitRegistrationRole, StoreError> {
	match value {
		"primary_worktree" => Ok(RepositoryGitRegistrationRole::PrimaryWorktree),
		"linked_worktree" => Ok(RepositoryGitRegistrationRole::LinkedWorktree),
		_ => Err(incompatible("stored admission Git registration role is invalid")),
	}
}

fn parse_observed_object_type(value: &str) -> Result<RepositoryObservedObjectType, StoreError> {
	match value {
		"directory" => Ok(RepositoryObservedObjectType::Directory),
		"regular_file" => Ok(RepositoryObservedObjectType::RegularFile),
		_ => Err(incompatible("stored admission observed object type is invalid")),
	}
}

fn parse_path_registration_role(value: &str) -> Result<RepositoryPathRegistrationRole, StoreError> {
	Ok(match value {
		"repository_root_component" => RepositoryPathRegistrationRole::RepositoryRootComponent,
		"repository_root" => RepositoryPathRegistrationRole::RepositoryRoot,
		"worktree_git_entry" => RepositoryPathRegistrationRole::WorktreeGitEntry,
		"git_directory_component" => RepositoryPathRegistrationRole::GitDirectoryComponent,
		"git_directory" => RepositoryPathRegistrationRole::GitDirectory,
		"git_common_directory_component" =>
			RepositoryPathRegistrationRole::GitCommonDirectoryComponent,
		"git_common_directory" => RepositoryPathRegistrationRole::GitCommonDirectory,
		"git_objects_directory_component" =>
			RepositoryPathRegistrationRole::GitObjectsDirectoryComponent,
		"git_objects_directory" => RepositoryPathRegistrationRole::GitObjectsDirectory,
		"git_refs_directory_component" => RepositoryPathRegistrationRole::GitRefsDirectoryComponent,
		"git_refs_directory" => RepositoryPathRegistrationRole::GitRefsDirectory,
		"git_common_directory_file" => RepositoryPathRegistrationRole::GitCommonDirectoryFile,
		"git_directory_backlink_file" => RepositoryPathRegistrationRole::GitDirectoryBacklinkFile,
		_ => return Err(incompatible("stored admission path role is invalid")),
	})
}

fn parse_ambiguity(value: &str) -> Result<RepositoryAmbiguity, StoreError> {
	Ok(match value {
		"stale" => RepositoryAmbiguity::Stale,
		"foreign" => RepositoryAmbiguity::Foreign,
		"replaced" => RepositoryAmbiguity::Replaced,
		"dirty" => RepositoryAmbiguity::Dirty,
		"rollback" => RepositoryAmbiguity::Rollback,
		"no_effect" => RepositoryAmbiguity::NoEffect,
		"incomplete" => RepositoryAmbiguity::Incomplete,
		"inconclusive" => RepositoryAmbiguity::Inconclusive,
		_ => return Err(incompatible("stored repository ambiguity is invalid")),
	})
}

fn phase_text(value: ManagedRepositoryPhase) -> &'static str {
	match value {
		ManagedRepositoryPhase::Allocated => "allocated",
		ManagedRepositoryPhase::Registered => "registered",
		ManagedRepositoryPhase::Ready => "ready",
		ManagedRepositoryPhase::Ambiguous(_) => "ambiguous",
	}
}

fn phase_ambiguity_text(value: ManagedRepositoryPhase) -> Option<&'static str> {
	match value {
		ManagedRepositoryPhase::Ambiguous(reason) => Some(ambiguity_text(reason)),
		_ => None,
	}
}

fn operation_kind_text(value: RepositoryOperationKind) -> &'static str {
	match value {
		RepositoryOperationKind::Register => "register",
		RepositoryOperationKind::WorktreeReady => "worktree_ready",
		RepositoryOperationKind::Commit => "commit",
	}
}

fn worktree_policy_text(value: WorktreeReadyPolicy) -> &'static str {
	match value {
		WorktreeReadyPolicy::ExactCleanWorktree => "exact_clean_worktree",
	}
}

fn git_registration_role_text(value: RepositoryGitRegistrationRole) -> &'static str {
	match value {
		RepositoryGitRegistrationRole::PrimaryWorktree => "primary_worktree",
		RepositoryGitRegistrationRole::LinkedWorktree => "linked_worktree",
	}
}

fn observed_object_type_text(value: RepositoryObservedObjectType) -> &'static str {
	match value {
		RepositoryObservedObjectType::Directory => "directory",
		RepositoryObservedObjectType::RegularFile => "regular_file",
	}
}

fn path_registration_role_text(value: RepositoryPathRegistrationRole) -> &'static str {
	match value {
		RepositoryPathRegistrationRole::RepositoryRootComponent => "repository_root_component",
		RepositoryPathRegistrationRole::RepositoryRoot => "repository_root",
		RepositoryPathRegistrationRole::WorktreeGitEntry => "worktree_git_entry",
		RepositoryPathRegistrationRole::GitDirectoryComponent => "git_directory_component",
		RepositoryPathRegistrationRole::GitDirectory => "git_directory",
		RepositoryPathRegistrationRole::GitCommonDirectoryComponent =>
			"git_common_directory_component",
		RepositoryPathRegistrationRole::GitCommonDirectory => "git_common_directory",
		RepositoryPathRegistrationRole::GitObjectsDirectoryComponent =>
			"git_objects_directory_component",
		RepositoryPathRegistrationRole::GitObjectsDirectory => "git_objects_directory",
		RepositoryPathRegistrationRole::GitRefsDirectoryComponent => "git_refs_directory_component",
		RepositoryPathRegistrationRole::GitRefsDirectory => "git_refs_directory",
		RepositoryPathRegistrationRole::GitCommonDirectoryFile => "git_common_directory_file",
		RepositoryPathRegistrationRole::GitDirectoryBacklinkFile => "git_directory_backlink_file",
	}
}

fn ambiguity_text(value: RepositoryAmbiguity) -> &'static str {
	match value {
		RepositoryAmbiguity::Stale => "stale",
		RepositoryAmbiguity::Foreign => "foreign",
		RepositoryAmbiguity::Replaced => "replaced",
		RepositoryAmbiguity::Dirty => "dirty",
		RepositoryAmbiguity::Rollback => "rollback",
		RepositoryAmbiguity::NoEffect => "no_effect",
		RepositoryAmbiguity::Incomplete => "incomplete",
		RepositoryAmbiguity::Inconclusive => "inconclusive",
	}
}

fn registration_evidence_text(value: &RegistrationEvidence) -> &'static str {
	match value {
		RegistrationEvidence::ExactReciprocal(_) => "exact_reciprocal",
		RegistrationEvidence::NoEffect => "no_effect",
		RegistrationEvidence::MissingReciprocal => "missing_reciprocal",
		RegistrationEvidence::Stale => "stale",
		RegistrationEvidence::Foreign => "foreign",
		RegistrationEvidence::Replaced => "replaced",
		RegistrationEvidence::Dirty => "dirty",
		RegistrationEvidence::Rollback => "rollback",
		RegistrationEvidence::Inconclusive => "inconclusive",
		RegistrationEvidence::Unavailable => "unavailable",
	}
}

fn worktree_ready_evidence_text(value: &WorktreeReadyEvidence) -> &'static str {
	match value {
		WorktreeReadyEvidence::Exact(_) => "exact",
		WorktreeReadyEvidence::NoEffect => "no_effect",
		WorktreeReadyEvidence::Incomplete => "incomplete",
		WorktreeReadyEvidence::Stale => "stale",
		WorktreeReadyEvidence::Foreign => "foreign",
		WorktreeReadyEvidence::Replaced => "replaced",
		WorktreeReadyEvidence::Dirty => "dirty",
		WorktreeReadyEvidence::Rollback => "rollback",
		WorktreeReadyEvidence::Inconclusive => "inconclusive",
		WorktreeReadyEvidence::Unavailable => "unavailable",
	}
}

fn commit_evidence_text(value: &CommitEvidence) -> &'static str {
	match value {
		CommitEvidence::Exact(_) => "exact",
		CommitEvidence::NoEffect => "no_effect",
		CommitEvidence::Incomplete => "incomplete",
		CommitEvidence::Stale => "stale",
		CommitEvidence::Foreign => "foreign",
		CommitEvidence::Replaced => "replaced",
		CommitEvidence::Dirty => "dirty",
		CommitEvidence::Rollback => "rollback",
		CommitEvidence::Inconclusive => "inconclusive",
		CommitEvidence::Unavailable => "unavailable",
	}
}

fn path_text(path: &PersistedAbsolutePath) -> Result<String, StoreError> {
	path.as_path()
		.to_str()
		.map(str::to_owned)
		.ok_or(StoreError::InvalidInput("managed-repository path is not UTF-8"))
}

fn optional_path_text(path: Option<&PersistedAbsolutePath>) -> Result<Option<String>, StoreError> {
	path.map(path_text).transpose()
}

fn observation_path_text(path: &RepositoryObservationPath) -> Result<String, StoreError> {
	path.as_path()
		.to_str()
		.map(str::to_owned)
		.ok_or(StoreError::InvalidInput("repository observation path is not UTF-8"))
}

fn parse_path(value: &str) -> Result<PersistedAbsolutePath, StoreError> {
	PersistedAbsolutePath::new(PathBuf::from(value)).map_err(StoreError::from)
}

fn parse_stored_path(value: &str) -> Result<PersistedAbsolutePath, StoreError> {
	PersistedAbsolutePath::new(PathBuf::from(value))
		.map_err(|_| incompatible("stored admission path is invalid"))
}

fn parse_observation_path(value: &str) -> Result<RepositoryObservationPath, StoreError> {
	RepositoryObservationPath::new(PathBuf::from(value))
		.map_err(|_| incompatible("stored admission observation path is invalid"))
}

fn parse_optional_stored_path(
	value: &Value,
	key: &str,
) -> Result<Option<PersistedAbsolutePath>, StoreError> {
	required_optional_str(value, key)?.map(parse_stored_path).transpose()
}

fn next_generation(value: u64) -> Result<u64, StoreError> {
	value.checked_add(1).ok_or_else(|| incompatible("managed-repository generation overflowed"))
}

fn generation_i64(value: u64) -> Result<i64, StoreError> {
	i64::try_from(value)
		.map_err(|_| incompatible("managed-repository generation exceeds PostgreSQL"))
}

fn required_value<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value.get(key).ok_or_else(|| incompatible("repository document is missing a field"))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	required_value(value, key)?
		.as_str()
		.ok_or_else(|| incompatible("repository document string field is invalid"))
}

fn required_optional_str<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match required_value(value, key)? {
		Value::Null => Ok(None),
		Value::String(value) => Ok(Some(value.as_str())),
		_ => Err(incompatible("repository document optional string field is invalid")),
	}
}

fn required_u64(value: &Value, key: &str) -> Result<u64, StoreError> {
	required_value(value, key)?
		.as_u64()
		.ok_or_else(|| incompatible("repository document unsigned field is invalid"))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	required_value(value, key)?
		.as_i64()
		.ok_or_else(|| incompatible("repository document integer field is invalid"))
}

fn incompatible(message: &'static str) -> StoreError {
	StoreError::Incompatible(message.into())
}
