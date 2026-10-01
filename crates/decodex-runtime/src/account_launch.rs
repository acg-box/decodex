//! Runtime-owned SQLite authorization and bounded process-capacity composition.

pub(crate) mod api_reset_card;
pub(crate) mod process;

mod activation_policy;
mod agent_process;
#[cfg(target_os = "macos")] mod macos_attested_spawn;
mod protocol;
mod reset_card_types;

pub(crate) use activation_policy::read_activation_policy;

#[cfg(all(test, unix))]
pub(crate) use agent_process::native_tests::account_nudge::{
	serve_notification as serve_native_nudge_fixture,
	serve_notification_with_gate as serve_native_nudge_with_gate,
};

pub(crate) use api_reset_card::ApiResetCardRuntime;

pub(crate) use process::{AttestedAppServerLaunch, AttestedAppServerProfile, AttestedProcessChild};

pub(crate) use reset_card_types::{
	ResetCardFailureCode, ResetCardInventoryObservation, ResetCardInventoryView,
	ResetCardObservationFailure, ResetCardOperationStatus, ResetCardPreparation,
	ResetCardServiceError,
};

use std::{
	fmt::{Debug, Formatter},
	sync::{
		Arc, Mutex, OnceLock, PoisonError, Weak,
		atomic::{AtomicU16, Ordering},
	},
};

use crate::account_launch::process::QuarantineSlotLease;

use decodex_core::AccountId;

const MAX_RUNNER_CAPACITY: u16 = 64;

/// Runtime-private capacity shared by live runners and quarantined cleanup.
///
/// ```compile_fail
/// use decodex_runtime::RunnerCapacity;
/// let _ = RunnerCapacity::daemon();
/// ```
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

struct CapacityInner {
	limit: u16,
	active: AtomicU16,
	quarantine: Arc<process::ProcessQuarantine>,
}

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
