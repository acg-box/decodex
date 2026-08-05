//! Runtime composition for the accepted managed-repository owners.
//!
//! This module adds no authority of its own. PostgreSQL remains current-state authority, the
//! executor remains the sole local effect owner, and the saga remains the only foreground and
//! restart sequencing path.

use std::sync::Arc;

use decodex_core::{
	AllocateRepositoryCommand, BeginCommitCommand, BeginRegistrationCommand,
	BeginWorktreeReadyCommand, ManagedRepositoryFacts, ManagedRepositoryId,
	RepositoryAdmissionFacts, RepositoryEvidenceId,
};
use decodex_postgres::{PostgresStore, RepositoryAdmissionOutcome, StoreError};
use tokio::sync::Mutex;

use crate::{
	ManagedRepositoryRestartOutcome, ManagedRepositorySagaOutcome,
	managed_repository_executor::{
		AcquisitionFailure, AllocationAcquisitionRequest, ExecutionFailure,
		ManagedRepositoryExecutor,
	},
	managed_repository_saga::ManagedRepositoryEffectSaga,
};

const MAX_RESTART_WORK: i64 = 256;

/// Typed managed-repository readiness projected by daemon bootstrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRepositoryReadiness {
	/// PostgreSQL, the pinned executor, and bounded restart reconciliation are ready.
	Ready,
	/// Managed-repository operations are intentionally not assembled.
	Disabled,
	/// Managed-repository operations are closed for one redacted reason.
	Unavailable(ManagedRepositoryUnavailableReason),
}

/// Redacted reason retained by an unavailable managed-repository capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRepositoryUnavailableReason {
	/// Verified PostgreSQL product state was unavailable.
	ProductStore,
	/// The exact pinned executor could not be opened and verified.
	Executor,
	/// Restart work could not be loaded, read back, or reconciled coherently.
	Reconciliation,
	/// Eligible restart work remained after the bounded startup pass.
	RestartWorkResidual,
}

/// Exact startup failure retained inside the runtime composition.
#[allow(dead_code)] // Debug retains the typed cause for fail-closed startup diagnostics.
#[derive(Debug)]
pub(crate) enum ManagedRepositoryStartupError {
	ExecutorOpen(ExecutionFailure),
	Reconciliation(StoreError),
	ResidualWork { processed: usize, limit: usize },
}

impl ManagedRepositoryStartupError {
	pub(crate) fn unavailable_reason(&self) -> ManagedRepositoryUnavailableReason {
		match self {
			Self::ExecutorOpen(_) => ManagedRepositoryUnavailableReason::Executor,
			Self::Reconciliation(_) => ManagedRepositoryUnavailableReason::Reconciliation,
			Self::ResidualWork { .. } => ManagedRepositoryUnavailableReason::RestartWorkResidual,
		}
	}
}

/// Immutable daemon-lifetime managed-repository capability.
pub(crate) enum ManagedRepositoryCapability {
	/// PostgreSQL, the executor, and bounded restart reconciliation are ready.
	Ready { _runtime: ManagedRepositoryRuntime },
	/// Managed-repository operations are intentionally not assembled.
	Disabled,
	/// Managed-repository operations are closed for one redacted reason.
	Unavailable {
		reason: ManagedRepositoryUnavailableReason,
		_error: Option<Arc<ManagedRepositoryStartupError>>,
	},
}

impl ManagedRepositoryCapability {
	pub(crate) fn unavailable(reason: ManagedRepositoryUnavailableReason) -> Self {
		Self::Unavailable { reason, _error: None }
	}

	pub(crate) fn startup_failed(error: ManagedRepositoryStartupError) -> Self {
		Self::Unavailable { reason: error.unavailable_reason(), _error: Some(Arc::new(error)) }
	}

	pub(crate) const fn readiness(&self) -> ManagedRepositoryReadiness {
		match self {
			Self::Ready { .. } => ManagedRepositoryReadiness::Ready,
			Self::Disabled => ManagedRepositoryReadiness::Disabled,
			Self::Unavailable { reason, .. } => ManagedRepositoryReadiness::Unavailable(*reason),
		}
	}
}

/// Fail-closed error at the runtime-only composition boundary.
#[allow(dead_code)] // Debug retains the typed cause across the private runtime boundary.
#[derive(Debug)]
pub(crate) enum ManagedRepositoryRuntimeError {
	Store(StoreError),
	Acquisition(AcquisitionFailure),
}

impl From<StoreError> for ManagedRepositoryRuntimeError {
	fn from(error: StoreError) -> Self {
		Self::Store(error)
	}
}

/// One daemon-owned composition of PostgreSQL, the accepted executor, and the effect saga.
#[derive(Clone)]
pub(crate) struct ManagedRepositoryRuntime {
	store: PostgresStore,
	saga: Arc<Mutex<ManagedRepositoryEffectSaga<ManagedRepositoryExecutor>>>,
}

impl ManagedRepositoryRuntime {
	/// Open the exact accepted executor and bind it to the bootstrapped PostgreSQL authority.
	pub(crate) async fn start(
		store: PostgresStore,
	) -> Result<Option<Self>, ManagedRepositoryStartupError> {
		if !store
			.has_managed_repository_authority()
			.await
			.map_err(ManagedRepositoryStartupError::Reconciliation)?
		{
			return Ok(None);
		}
		let executor = ManagedRepositoryExecutor::open()
			.map_err(ManagedRepositoryStartupError::ExecutorOpen)?;
		let saga = ManagedRepositoryEffectSaga::new(store.clone(), executor);
		let runtime = Self { store, saga: Arc::new(Mutex::new(saga)) };
		let processed = runtime
			.reconcile_restart_batch(MAX_RESTART_WORK)
			.await
			.map_err(ManagedRepositoryStartupError::Reconciliation)?
			.len();
		let residual = runtime
			.reconcile_restart_batch(1)
			.await
			.map_err(ManagedRepositoryStartupError::Reconciliation)?;
		if !residual.is_empty() {
			return Err(ManagedRepositoryStartupError::ResidualWork {
				processed,
				limit: MAX_RESTART_WORK as usize,
			});
		}
		Ok(Some(runtime))
	}

	/// Persist one immutable admission through PostgreSQL only.
	#[allow(dead_code)]
	pub(crate) async fn admit(
		&self,
		admission: &RepositoryAdmissionFacts,
	) -> Result<RepositoryAdmissionOutcome, StoreError> {
		self.store.admit_repository(admission).await
	}

	/// Acquire read-only executor evidence and atomically claim the allocation in PostgreSQL.
	#[allow(dead_code)]
	pub(crate) async fn allocate(
		&self,
		repository_id: &ManagedRepositoryId,
		admission: &RepositoryAdmissionFacts,
		command: &AllocateRepositoryCommand,
		evidence_id: RepositoryEvidenceId,
	) -> Result<ManagedRepositoryFacts, ManagedRepositoryRuntimeError> {
		let saga = self.saga.lock().await;
		let evidence = saga
			.effects()
			.acquire_allocation(AllocationAcquisitionRequest {
				admission,
				vacant_worktree_path: &command.worktree_path,
				evidence_id,
			})
			.map_err(ManagedRepositoryRuntimeError::Acquisition)?;
		self.store
			.allocate_repository(repository_id, command, &evidence)
			.await
			.map_err(ManagedRepositoryRuntimeError::Store)
	}

	/// Run the accepted Register preparation/dispatch/readback/reconciliation path.
	#[allow(dead_code)]
	pub(crate) async fn register(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &BeginRegistrationCommand,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		self.saga.lock().await.register(repository_id, command).await
	}

	/// Run the accepted WorktreeReady preparation/dispatch/readback/reconciliation path.
	#[allow(dead_code)]
	pub(crate) async fn make_worktree_ready(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &BeginWorktreeReadyCommand,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		self.saga.lock().await.make_worktree_ready(repository_id, command).await
	}

	/// Run the accepted Commit preparation/dispatch/readback/reconciliation path.
	#[allow(dead_code)]
	pub(crate) async fn commit(
		&self,
		repository_id: &ManagedRepositoryId,
		command: &BeginCommitCommand,
	) -> Result<ManagedRepositorySagaOutcome, StoreError> {
		self.saga.lock().await.commit(repository_id, command).await
	}

	/// Reconcile one explicitly bounded batch of committed restart work.
	async fn reconcile_restart_batch(
		&self,
		limit: i64,
	) -> Result<Vec<ManagedRepositoryRestartOutcome>, StoreError> {
		self.saga.lock().await.reconcile_restart(limit).await
	}
}
