//! Runtime-owned SQLite authorization and bounded process-capacity composition.

mod api_reset_card_disabled;
#[cfg(target_os = "macos")] mod macos_attested_spawn;
pub(crate) mod process;
mod protocol;
mod reset_card_types;

pub(crate) use api_reset_card_disabled::ApiResetCardRuntime;
pub(crate) use process::{AttestedAppServerLaunch, AttestedAppServerProfile, AttestedProcessChild};
pub(crate) use reset_card_types::{
	ResetCardFailureCode, ResetCardInventoryObservation, ResetCardInventoryView,
	ResetCardObservationFailure, ResetCardOperationStatus, ResetCardPreparation,
	ResetCardServiceError,
};

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
	sync::{
		Arc, Mutex, OnceLock, PoisonError, Weak,
		atomic::{AtomicU16, Ordering},
	},
};

use crate::account_launch::process::{
	CredentialVault, ExactThreadReconciler, ExactThreadReconciliation,
	ExactThreadReconciliationFailure, ExactThreadReconciliationResult, ProbeError,
	QuarantineSlotLease, ReadOnlyProbe, ReadOnlyProbeResult,
};
use decodex_codex::CapabilityCache;
use decodex_core::AccountId;
use decodex_database::SqliteStore;

const MAX_RUNNER_CAPACITY: u16 = 64;

/// Dormant explicit account observation composition; it contains no selector or routing policy.
///
/// Product capacity cannot be constructed or reserved outside this private owner:
///
/// ```compile_fail
/// use decodex_runtime::RunnerCapacity;
///
/// let _ = RunnerCapacity::daemon();
/// ```
#[derive(Clone)]
struct ManualAccountLauncher {
	store: SqliteStore,
	capacity: Arc<RunnerCapacity>,
}
impl ManualAccountLauncher {
	/// Bind the dormant composition to the daemon-owned SQLite authority.
	fn new(store: &SqliteStore) -> Result<Self, CapacityExhausted> {
		Ok(Self { store: store.clone(), capacity: RunnerCapacity::daemon()? })
	}

	#[cfg(test)]
	fn with_capacity(store: &SqliteStore, limit: u16) -> Self {
		Self {
			store: store.clone(),
			capacity: Arc::new(RunnerCapacity::try_with_limit(limit).unwrap()),
		}
	}

	/// Produce one post-cleanup observation for an explicitly selected account and revision.
	///
	/// SQLite is observed once before any process can spawn and again after the process is
	/// confirmed dead or its guard is quarantined. No database client or transaction spans the
	/// process or vault call. The returned value is not live-runner authority.
	async fn run_bound(
		&self,
		request: ManualAccountLaunchRequest,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
	) -> Result<ManualAccountLaunchResult, ManualAccountLaunchError> {
		let ManualAccountLaunchRequest { account_id, expected_revision, probe } = request;

		if probe.account_id() != &account_id {
			return Err(ManualAccountLaunchError::BindingMismatch);
		}
		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		let guard = self
			.capacity
			.reserve(account_id.clone(), expected_revision)
			.map_err(|_| ManualAccountLaunchError::CapacityExhausted)?;
		let observation = probe
			.run_mechanical_with_lifetime_guard(vault, cache, guard)
			.map_err(ManualAccountLaunchError::Probe)?;

		if observation.account_id != account_id {
			return Err(ManualAccountLaunchError::BindingMismatch);
		}
		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(ManualAccountLaunchResult { observation, account_revision: expected_revision })
	}

	/// Execute one exact reconciliation under an explicitly selected ready account.
	///
	/// This private composition performs no selection, persistence, turn dispatch, or restart
	/// replay. Durable account authority is checked before process mechanics and again after
	/// bounded cleanup.
	async fn reconcile_bound(
		&self,
		request: ManualReconciliationRequest,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
	) -> Result<ManualReconciliationResult, ManualAccountLaunchError> {
		let ManualReconciliationRequest { account_id, expected_revision, reconciler, operation } =
			request;

		if reconciler.account_id() != &account_id {
			return Err(ManualAccountLaunchError::BindingMismatch);
		}
		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		let guard = self
			.capacity
			.reserve(account_id.clone(), expected_revision)
			.map_err(|_| ManualAccountLaunchError::CapacityExhausted)?;
		let result = reconciler
			.run_mechanical_with_lifetime_guard(vault, cache, guard, operation)
			.map_err(ManualAccountLaunchError::Reconciliation)?;

		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(ManualReconciliationResult { result, account_id, account_revision: expected_revision })
	}

	#[cfg(test)]
	fn active_capacity(&self) -> u16 {
		self.capacity.active()
	}
}

/// Exact caller-provided identity and revision for one dormant manual observation.
struct ManualAccountLaunchRequest {
	account_id: AccountId,
	expected_revision: i64,
	probe: ReadOnlyProbe,
}
impl ManualAccountLaunchRequest {
	/// Construct an explicit request without selecting or discovering an account.
	fn new(
		account_id: AccountId,
		expected_revision: i64,
		probe: ReadOnlyProbe,
	) -> Result<Self, ManualAccountLaunchError> {
		if expected_revision < 1 {
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(Self { account_id, expected_revision, probe })
	}
}

/// Exact caller-provided account authority and one closed reconciliation operation.
struct ManualReconciliationRequest {
	account_id: AccountId,
	expected_revision: i64,
	reconciler: ExactThreadReconciler,
	operation: ExactThreadReconciliation,
}
impl ManualReconciliationRequest {
	fn new(
		account_id: AccountId,
		expected_revision: i64,
		reconciler: ExactThreadReconciler,
		operation: ExactThreadReconciliation,
	) -> Result<Self, ManualAccountLaunchError> {
		if expected_revision < 1 {
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(Self { account_id, expected_revision, reconciler, operation })
	}
}
/// Non-live observation produced only after exact final product-store revalidation and cleanup.
struct ManualAccountLaunchResult {
	observation: ReadOnlyProbeResult,
	account_revision: i64,
}
impl ManualAccountLaunchResult {
	/// Mechanical Codex observation validated against the authorized account.
	const fn observation(&self) -> &ReadOnlyProbeResult {
		&self.observation
	}

	/// Exact positive account-readiness revision re-observed after cleanup.
	const fn account_revision(&self) -> i64 {
		self.account_revision
	}
}
impl Debug for ManualAccountLaunchResult {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ManualAccountLaunchResult")
			.field("account_id", &self.observation.account_id)
			.field("account_revision", &self.account_revision)
			.field("process_id", &self.observation.process_id)
			.finish_non_exhaustive()
	}
}

/// Non-live reconciliation result released only after exact final account revalidation.
struct ManualReconciliationResult {
	result: ExactThreadReconciliationResult,
	account_id: AccountId,
	account_revision: i64,
}
impl Debug for ManualReconciliationResult {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		let operation = match self.result {
			ExactThreadReconciliationResult::List(_) => "list",
			ExactThreadReconciliationResult::Read(_) => "read",
			ExactThreadReconciliationResult::Archive(_) => "archive",
		};

		formatter
			.debug_struct("ManualReconciliationResult")
			.field("account_id", &self.account_id)
			.field("account_revision", &self.account_revision)
			.field("operation", &operation)
			.finish_non_exhaustive()
	}
}
/// Closed manual launch failure without database, account, credential, or provider text.
#[derive(Debug)]
enum ManualAccountLaunchError {
	/// The product store could not provide account authority.
	ProductStateUnavailable,
	/// The exact account revision was stale, missing, or not ready.
	ReadinessRejected,
	/// The runtime-private daemon capacity is occupied, including quarantined cleanup.
	CapacityExhausted,
	/// Mechanical probe binding contradicted the durably authorized account.
	BindingMismatch,
	/// Bounded process mechanics failed.
	Probe(ProbeError),
	/// Exact reconciliation mechanics failed closed.
	Reconciliation(ExactThreadReconciliationFailure),
}
impl Display for ManualAccountLaunchError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Probe(error) => Display::fmt(error, formatter),
			Self::Reconciliation(error) => write!(formatter, "{error:?}"),
			_ => write!(formatter, "{self:?}"),
		}
	}
}
impl Error for ManualAccountLaunchError {}

pub(crate) struct RunnerCapacity {
	inner: Arc<CapacityInner>,
}
impl RunnerCapacity {
	pub(crate) fn daemon() -> Result<Arc<Self>, CapacityExhausted> {
		static DAEMON: OnceLock<Mutex<Weak<RunnerCapacity>>> = OnceLock::new();

		let mut daemon = DAEMON
			.get_or_init(|| Mutex::new(Weak::new()))
			.lock()
			.unwrap_or_else(PoisonError::into_inner);

		if let Some(capacity) = daemon.upgrade() {
			return Ok(capacity);
		}

		let capacity = Arc::new(Self::try_with_limit(MAX_RUNNER_CAPACITY)?);

		*daemon = Arc::downgrade(&capacity);

		Ok(capacity)
	}

	fn try_with_limit(limit: u16) -> Result<Self, CapacityExhausted> {
		assert!((1..=MAX_RUNNER_CAPACITY).contains(&limit));

		Ok(Self {
			inner: Arc::new(CapacityInner {
				limit,
				active: AtomicU16::new(0),
				quarantine: process::ProcessQuarantine::try_new().map_err(|_| CapacityExhausted)?,
			}),
		})
	}

	pub(crate) fn reserve(
		&self,
		account_id: AccountId,
		account_revision: i64,
	) -> Result<RunnerPermit, CapacityExhausted> {
		if account_revision < 1 {
			return Err(CapacityExhausted);
		}

		let mut active = self.inner.active.load(Ordering::Acquire);

		loop {
			if active >= self.inner.limit {
				return Err(CapacityExhausted);
			}

			match self.inner.active.compare_exchange_weak(
				active,
				active + 1,
				Ordering::AcqRel,
				Ordering::Acquire,
			) {
				Ok(_) => break,
				Err(observed) => active = observed,
			}
		}

		let Some(quarantine_slot) = self.inner.quarantine.reserve_slot() else {
			self.inner.active.fetch_sub(1, Ordering::AcqRel);

			return Err(CapacityExhausted);
		};

		Ok(RunnerPermit {
			capacity: Arc::clone(&self.inner),
			account_id,
			account_revision,
			quarantine: Arc::clone(&self.inner.quarantine),
			quarantine_slot,
		})
	}

	#[cfg(test)]
	fn active(&self) -> u16 {
		self.inner.active.load(Ordering::Acquire)
	}
}

struct CapacityInner {
	limit: u16,
	active: AtomicU16,
	quarantine: Arc<process::ProcessQuarantine>,
}

pub(crate) struct RunnerPermit {
	capacity: Arc<CapacityInner>,
	account_id: AccountId,
	account_revision: i64,
	quarantine: Arc<process::ProcessQuarantine>,
	quarantine_slot: QuarantineSlotLease,
}
impl RunnerPermit {
	fn quarantine(&mut self) -> (Arc<process::ProcessQuarantine>, usize) {
		self.quarantine_slot.mark_installed();

		(Arc::clone(&self.quarantine), self.quarantine_slot.index())
	}

	#[cfg(test)]
	fn use_quarantine_for_test(&mut self, quarantine: &Arc<process::ProcessQuarantine>) {
		self.quarantine_slot = quarantine.reserve_slot().expect("test quarantine has a free slot");
		self.quarantine = Arc::clone(quarantine);
	}
}
impl Debug for RunnerPermit {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("RunnerPermit")
			.field("account_id", &self.account_id)
			.field("account_revision", &self.account_revision)
			.finish_non_exhaustive()
	}
}
impl Drop for RunnerPermit {
	fn drop(&mut self) {
		let previous = self.capacity.active.fetch_sub(1, Ordering::AcqRel);

		debug_assert!(previous > 0, "owned runner capacity cannot underflow");
	}
}

#[derive(Debug)]
pub(crate) struct CapacityExhausted;

#[cfg(test)]
mod tests {
	use std::{ptr, sync::Arc};

	use crate::account_launch::{CapacityExhausted, RunnerCapacity};
	use decodex_core::AccountId;

	fn account(suffix: u8) -> AccountId {
		AccountId::new(format!("10000000-0000-4000-8000-{suffix:012x}")).unwrap()
	}

	#[test]
	fn one_private_counter_rejects_parallel_capacity() {
		let capacity = RunnerCapacity::try_with_limit(1).unwrap();
		let permit = capacity.reserve(account(1), 7).unwrap();

		assert_eq!(capacity.active(), 1);
		assert!(matches!(capacity.reserve(account(2), 8), Err(CapacityExhausted)));

		drop(permit);

		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn daemon_registry_reuses_only_the_live_private_process_authority() {
		let first = RunnerCapacity::daemon().unwrap();
		let second = RunnerCapacity::daemon().unwrap();
		let weak = Arc::downgrade(&first);

		assert!(ptr::eq(Arc::as_ptr(&first), Arc::as_ptr(&second)));

		drop(first);
		drop(second);

		assert!(weak.upgrade().is_none());
		assert_eq!(RunnerCapacity::daemon().unwrap().active(), 0);
	}

	#[test]
	fn capacity_and_cleanup_slots_share_one_hard_bound() {
		let capacity = RunnerCapacity::try_with_limit(64).unwrap();
		let permits = (0..64)
			.map(|index| capacity.reserve(account(index), i64::from(index) + 1).unwrap())
			.collect::<Vec<_>>();

		assert_eq!(capacity.active(), 64);
		assert!(matches!(capacity.reserve(account(65), 66), Err(CapacityExhausted)));

		drop(permits);

		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn restart_constructs_no_persisted_capacity_or_assignment() {
		let first_process = RunnerCapacity::try_with_limit(1).unwrap();
		let permit = first_process.reserve(account(1), 3).unwrap();

		assert_eq!(first_process.active(), 1);

		drop(permit);

		let restarted_process = RunnerCapacity::try_with_limit(1).unwrap();

		assert_eq!(restarted_process.active(), 0);
	}
}
