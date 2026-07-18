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
use decodex_postgres::{
	PostgresStore, RepositoryAdmissionOutcome, StoreError,
};
use tokio::sync::Mutex;

use crate::{
	ManagedRepositoryRestartOutcome, ManagedRepositorySagaOutcome,
	managed_repository_executor::{
		AcquisitionFailure, AllocationAcquisitionRequest, ManagedRepositoryExecutor,
	},
	managed_repository_saga::ManagedRepositoryEffectSaga,
};

const MAX_RESTART_WORK: i64 = 256;

/// Fail-closed error at the runtime-only composition boundary.
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
	pub(crate) fn open(store: PostgresStore) -> Option<Self> {
		let executor = ManagedRepositoryExecutor::open().ok()?;
		let saga = ManagedRepositoryEffectSaga::new(store.clone(), executor);
		Some(Self { store, saga: Arc::new(Mutex::new(saga)) })
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

	/// Reconcile all bounded committed restart work before the daemon begins serving.
	pub(crate) async fn reconcile_restart(
		&self,
	) -> Result<Vec<ManagedRepositoryRestartOutcome>, StoreError> {
		self.saga.lock().await.reconcile_restart(MAX_RESTART_WORK).await
	}
}
