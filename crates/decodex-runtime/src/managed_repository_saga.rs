//! Application-level durable managed-repository effect saga.
//!
//! PostgreSQL remains the only current-state authority. This owner sequences a fresh
//! post-COMMIT affine receipt through exactly one effect attempt, operation-specific readback,
//! and durable reconciliation. Restart enters below the receipt boundary and is readback-only.

use decodex_core::{
	BeginCommitCommand, BeginRegistrationCommand, BeginWorktreeReadyCommand, CommitEvidence,
	CanonicalOperationPayload, ManagedRepositoryFacts, ManagedRepositoryId,
	ManagedRepositoryPhase, OperationView, RegistrationEvidence, RepositoryAdmissionFacts,
	RepositoryEvidenceId, RepositoryOperationId, RepositoryOperationState, WorktreeReadyEvidence,
};
use decodex_postgres::{
	PostgresStore, RepositoryDispatchReceipt, RepositoryPreparationOutcome,
	RepositoryReadbackWork, RepositoryReconciliationOutcome, RepositoryRestartState, StoreError,
};

use crate::managed_repository_executor::{
	ExecutionAttempt, ExecutionFailure, ManagedRepositoryExecutor,
};

/// Closed, non-authoritative observation of the sole in-process effect attempt.
///
/// Durable readback and PostgreSQL reconciliation decide the operation result. This observation
/// exists only for bounded diagnostics and can never authorize another attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryDispatchObservation {
	/// The authorized invocation returned; exact completion still requires readback.
	InvocationReturned,
	/// The receipt was consumed before the external invocation began.
	ConsumedWithoutInvocation(RepositoryDispatchFailure),
	/// Invocation may have begun or produced state, but did not return a trusted success.
	InvocationUncertain(RepositoryDispatchFailure),
}

/// Stable application classification for a consumed local repository attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryDispatchFailure {
	AlreadyAttempted,
	InvalidDescriptor,
	UnsupportedContract,
	PathUnavailable,
	UnsafeOwner,
	Replaced,
	ForeignRepository,
	UnsupportedRepository,
	TargetOccupied,
	PrivateIndexConflict,
	ExecutableUnavailable,
	SpawnFailed,
	StdinFailed,
	TimedOut,
	OutputLimit,
	Exited(i32),
	Signaled(i32),
	UnexpectedOutput,
	PreconditionMismatch,
}

/// Operation-specific evidence returned by a strictly read-only effect observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryReadbackEvidence {
	Registration(RegistrationEvidence),
	WorktreeReady(WorktreeReadyEvidence),
	Commit(CommitEvidence),
}

/// Minimal effect/readback port for the local executor and later provider composition.
///
/// Only `dispatch` accepts the affine receipt. `readback` accepts data and an observation identity,
/// so neither restart nor a provider implementation can reconstruct execution authority.
pub trait ManagedRepositoryEffectPort {
	fn dispatch(
		&mut self,
		receipt: RepositoryDispatchReceipt,
		admission: &RepositoryAdmissionFacts,
	) -> RepositoryDispatchObservation;

	fn readback(
		&self,
		work: &RepositoryReadbackWork,
		admission: &RepositoryAdmissionFacts,
		evidence_id: RepositoryEvidenceId,
	) -> RepositoryReadbackEvidence;
}

/// Result of one foreground saga invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedRepositorySagaOutcome {
	/// The globally immutable descriptor already existed exactly; no receipt or dispatch occurred.
	ExistingExact(OperationView),
	/// A fresh receipt was consumed once and its resulting readback was durably reconciled.
	Reconciled {
		dispatch: RepositoryDispatchObservation,
		outcome: RepositoryReconciliationOutcome,
	},
}

/// Result of one readback-only restart reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRepositoryRestartOutcome {
	pub operation_id: RepositoryOperationId,
	pub outcome: RepositoryReconciliationOutcome,
}

/// The application owner that composes PostgreSQL preparation, one effect, and reconciliation.
pub struct ManagedRepositoryEffectSaga<P> {
	store: PostgresStore,
	effects: P,
}

impl<P> ManagedRepositoryEffectSaga<P>
where
	P: ManagedRepositoryEffectPort,
{
	pub fn new(store: PostgresStore, effects: P) -> Self {
		Self { store, effects }
	}

	pub fn effects(&self) -> &P {
		&self.effects
	}

	pub fn effects_mut(&mut self) -> &mut P {
		&mut self.effects
	}

	pub async fn register(
		&mut self,
		repository_id: &ManagedRepositoryId,
		command: &BeginRegistrationCommand,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		let prepared = self.store.prepare_registration(repository_id, command).await?;
		self.consume_preparation(prepared).await
	}

	pub async fn make_worktree_ready(
		&mut self,
		repository_id: &ManagedRepositoryId,
		command: &BeginWorktreeReadyCommand,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		let prepared = self.store.prepare_worktree_ready(repository_id, command).await?;
		self.consume_preparation(prepared).await
	}

	pub async fn commit(
		&mut self,
		repository_id: &ManagedRepositoryId,
		command: &BeginCommitCommand,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		let prepared = self.store.prepare_commit(repository_id, command).await?;
		self.consume_preparation(prepared).await
	}

	/// Reconcile bounded committed work after process start without preparing or dispatching.
	pub async fn reconcile_restart(
		&self,
		limit: i64,
	) -> Result<Vec<ManagedRepositoryRestartOutcome>, StoreError> {
		let states = self.store.load_repository_restart_work(limit).await?;
		let mut outcomes = Vec::with_capacity(states.len());
		for state in states {
			outcomes.push(self.reconcile_restart_state(state).await?);
		}
		Ok(outcomes)
	}

	async fn consume_preparation(
		&mut self,
		prepared: RepositoryPreparationOutcome,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		let (operation, receipt) = match prepared {
			RepositoryPreparationOutcome::ExistingExact(operation, _) =>
				return Ok(ManagedRepositorySagaOutcome::ExistingExact(operation)),
			RepositoryPreparationOutcome::Prepared { operation, receipt } => (operation, receipt),
		};

		// Deliberately reload after acknowledged COMMIT. If this readback cannot establish the exact
		// committed fence, dropping the affine receipt fails closed and no effect occurs.
		let repository = self
			.store
			.read_managed_repository(&operation.descriptor.repository_id)
			.await?
			.ok_or_else(|| incompatible("prepared repository is absent on durable readback"))?;
		validate_prepared_readback(&repository, &operation)?;

		let work = readback_work(&operation)?;
		let dispatch = self.effects.dispatch(receipt, &repository.admission);
		let outcome = self.reconcile(&work, &repository.admission).await?;
		Ok(ManagedRepositorySagaOutcome::Reconciled { dispatch, outcome })
	}

	async fn reconcile_restart_state(
		&self,
		state: RepositoryRestartState,
	) -> Result<ManagedRepositoryRestartOutcome, StoreError> {
		validate_prepared_readback(&state.repository, &state.operation)?;
		if state.allocation_evidence.admission_descriptor()
			!= state.repository.admission.descriptor()
		{
			return Err(incompatible("restart allocation evidence contradicts admission"));
		}
		let operation_id = state.operation.descriptor.operation_id.clone();
		let outcome = self.reconcile(&state.readback, &state.repository.admission).await?;
		Ok(ManagedRepositoryRestartOutcome { operation_id, outcome })
	}

	async fn reconcile(
		&self,
		work: &RepositoryReadbackWork,
		admission: &RepositoryAdmissionFacts,
	) -> Result<RepositoryReconciliationOutcome, StoreError> {
		let evidence_id = self.store.issue_repository_readback_evidence_id().await?;
		match self.effects.readback(work, admission, evidence_id) {
			RepositoryReadbackEvidence::Registration(evidence) => {
				let operation_id = &registration_work(work)?.descriptor.operation_id;
				self.store.reconcile_registration(operation_id, &evidence).await
			},
			RepositoryReadbackEvidence::WorktreeReady(evidence) => {
				let operation_id = &worktree_ready_work(work)?.descriptor.operation_id;
				self.store.reconcile_worktree_ready(operation_id, &evidence).await
			},
			RepositoryReadbackEvidence::Commit(evidence) => {
				let operation_id = &commit_work(work)?.descriptor.operation_id;
				self.store.reconcile_commit(operation_id, &evidence).await
			},
		}
	}
}

impl ManagedRepositoryEffectPort for ManagedRepositoryExecutor {
	fn dispatch(
		&mut self,
		receipt: RepositoryDispatchReceipt,
		admission: &RepositoryAdmissionFacts,
	) -> RepositoryDispatchObservation {
		let kind = receipt.descriptor().kind;
		let attempt = match kind {
			decodex_core::RepositoryOperationKind::Register =>
				self.execute_register(receipt, admission),
			decodex_core::RepositoryOperationKind::WorktreeReady =>
				self.execute_worktree_ready(receipt, admission),
			decodex_core::RepositoryOperationKind::Commit => self.execute_commit(receipt, admission),
		};
		map_attempt(attempt)
	}

	fn readback(
		&self,
		work: &RepositoryReadbackWork,
		admission: &RepositoryAdmissionFacts,
		evidence_id: RepositoryEvidenceId,
	) -> RepositoryReadbackEvidence {
		match work {
			RepositoryReadbackWork::Registration(request) => RepositoryReadbackEvidence::Registration(
				self.read_registration(request, admission, evidence_id),
			),
			RepositoryReadbackWork::WorktreeReady(request) =>
				RepositoryReadbackEvidence::WorktreeReady(self.read_worktree_ready(
					request,
					admission,
					evidence_id,
				)),
			RepositoryReadbackWork::Commit(request) => RepositoryReadbackEvidence::Commit(
				self.read_commit(request, admission, evidence_id),
			),
		}
	}
}

fn validate_prepared_readback(
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
	if operation.state != RepositoryOperationState::PossiblyEffected
		|| repository.active_operation.as_ref() != Some(&operation.descriptor.operation_id)
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
		return Err(incompatible("durable preparation readback contradicts operation fence"));
	}
	Ok(())
}

fn readback_work(operation: &OperationView) -> Result<RepositoryReadbackWork, StoreError> {
	Ok(match operation.descriptor.kind {
		decodex_core::RepositoryOperationKind::Register => RepositoryReadbackWork::Registration(
			decodex_core::registration_readback_request(operation)?,
		),
		decodex_core::RepositoryOperationKind::WorktreeReady =>
			RepositoryReadbackWork::WorktreeReady(decodex_core::worktree_ready_readback_request(
				operation,
			)?),
		decodex_core::RepositoryOperationKind::Commit =>
			RepositoryReadbackWork::Commit(decodex_core::commit_readback_request(operation)?),
	})
}

fn registration_work(
	work: &RepositoryReadbackWork,
) -> Result<&decodex_core::RegistrationReadbackRequest, StoreError> {
	match work {
		RepositoryReadbackWork::Registration(request) => Ok(request),
		_ => Err(incompatible("effect port returned registration evidence for another operation")),
	}
}

fn worktree_ready_work(
	work: &RepositoryReadbackWork,
) -> Result<&decodex_core::WorktreeReadyReadbackRequest, StoreError> {
	match work {
		RepositoryReadbackWork::WorktreeReady(request) => Ok(request),
		_ => Err(incompatible("effect port returned readiness evidence for another operation")),
	}
}

fn commit_work(
	work: &RepositoryReadbackWork,
) -> Result<&decodex_core::CommitReadbackRequest, StoreError> {
	match work {
		RepositoryReadbackWork::Commit(request) => Ok(request),
		_ => Err(incompatible("effect port returned commit evidence for another operation")),
	}
}

fn map_attempt(attempt: ExecutionAttempt) -> RepositoryDispatchObservation {
	match attempt {
		ExecutionAttempt::CompletedInvocation => RepositoryDispatchObservation::InvocationReturned,
		ExecutionAttempt::ConsumedWithoutInvocation(error) =>
			RepositoryDispatchObservation::ConsumedWithoutInvocation(map_failure(error)),
		ExecutionAttempt::InvocationFailed(error) =>
			RepositoryDispatchObservation::InvocationUncertain(map_failure(error)),
	}
}

fn map_failure(error: ExecutionFailure) -> RepositoryDispatchFailure {
	match error {
		ExecutionFailure::AlreadyAttempted => RepositoryDispatchFailure::AlreadyAttempted,
		ExecutionFailure::InvalidDescriptor => RepositoryDispatchFailure::InvalidDescriptor,
		ExecutionFailure::UnsupportedContract => RepositoryDispatchFailure::UnsupportedContract,
		ExecutionFailure::PathUnavailable => RepositoryDispatchFailure::PathUnavailable,
		ExecutionFailure::UnsafeOwner => RepositoryDispatchFailure::UnsafeOwner,
		ExecutionFailure::Replaced => RepositoryDispatchFailure::Replaced,
		ExecutionFailure::ForeignRepository => RepositoryDispatchFailure::ForeignRepository,
		ExecutionFailure::UnsupportedRepository => RepositoryDispatchFailure::UnsupportedRepository,
		ExecutionFailure::TargetOccupied => RepositoryDispatchFailure::TargetOccupied,
		ExecutionFailure::PrivateIndexConflict => RepositoryDispatchFailure::PrivateIndexConflict,
		ExecutionFailure::GitUnavailable => RepositoryDispatchFailure::ExecutableUnavailable,
		ExecutionFailure::SpawnFailed => RepositoryDispatchFailure::SpawnFailed,
		ExecutionFailure::StdinFailed => RepositoryDispatchFailure::StdinFailed,
		ExecutionFailure::TimedOut => RepositoryDispatchFailure::TimedOut,
		ExecutionFailure::OutputLimit => RepositoryDispatchFailure::OutputLimit,
		ExecutionFailure::Exited(code) => RepositoryDispatchFailure::Exited(code),
		ExecutionFailure::Signaled(signal) => RepositoryDispatchFailure::Signaled(signal),
		ExecutionFailure::UnexpectedOutput => RepositoryDispatchFailure::UnexpectedOutput,
		ExecutionFailure::PreconditionMismatch => RepositoryDispatchFailure::PreconditionMismatch,
	}
}

fn incompatible(reason: &str) -> StoreError {
	StoreError::Incompatible(reason.to_owned())
}
