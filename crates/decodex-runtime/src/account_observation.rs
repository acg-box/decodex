//! Daemon-owned background observation for every independent account.

use std::{
	collections::{HashMap, HashSet},
	future,
	sync::Arc,
	time::Duration,
};

use decodex_core::{AccountId, AccountLifecycleReadiness};
use tokio::{
	sync::{Notify, RwLock, watch},
	task::{Id as TaskId, JoinSet},
	time::{self, MissedTickBehavior},
};

use crate::{
	account_launch::{ResetCardInventoryObservation, ResetCardRuntime, ResetCardServiceError},
	account_profile::{
		AccountProfileRefreshStatus, AccountProfileRuntime, AccountProfileRuntimeResult,
	},
	account_service::{AccountLifecycleError, AccountService},
};

const OBSERVATION_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct CachedResetCardInventory {
	account_revision: i64,
	result: Result<ResetCardInventoryObservation, ResetCardServiceError>,
}

#[derive(Default)]
struct AccountObservationState {
	reset_cards: HashMap<AccountId, CachedResetCardInventory>,
	profiles: HashMap<AccountId, AccountProfileRefreshStatus>,
}

struct AccountObservationOutcome {
	account_id: AccountId,
	requested_revision: i64,
	reset_cards: Option<Result<ResetCardInventoryObservation, ResetCardServiceError>>,
	profile: Option<AccountProfileRefreshStatus>,
}

impl AccountObservationState {
	fn retain_current(&mut self, current: &HashMap<AccountId, i64>) {
		self.reset_cards.retain(|account_id, cached| {
			current.get(account_id).is_some_and(|revision| *revision == cached.account_revision)
		});
		self.profiles.retain(|account_id, status| {
			current.get(account_id).is_some_and(|revision| *revision == status.account_revision)
		});
	}

	fn insert(&mut self, observation: AccountObservationOutcome) {
		if let Some(reset_cards) = observation.reset_cards {
			let inventory_revision = reset_cards
				.as_ref()
				.ok()
				.map(inventory_revision)
				.unwrap_or(observation.requested_revision);
			self.reset_cards.insert(
				observation.account_id.clone(),
				CachedResetCardInventory {
					account_revision: inventory_revision,
					result: reset_cards,
				},
			);
		}
		if let Some(profile) = observation.profile {
			self.profiles.insert(observation.account_id, profile);
		}
	}
}

/// One daemon-lifecycle observer that refreshes independent accounts concurrently.
#[derive(Clone)]
pub(crate) struct AccountObservationService {
	accounts: Arc<AccountService>,
	profiles: Option<AccountProfileRuntime>,
	reset_cards: Option<ResetCardRuntime>,
	state: Arc<RwLock<AccountObservationState>>,
	refresh_requested: Arc<Notify>,
}

impl AccountObservationService {
	pub(crate) fn new(
		accounts: Arc<AccountService>,
		profiles: Option<AccountProfileRuntime>,
		reset_cards: Option<ResetCardRuntime>,
	) -> Self {
		Self {
			accounts,
			profiles,
			reset_cards,
			state: Arc::new(RwLock::new(AccountObservationState::default())),
			refresh_requested: Arc::new(Notify::new()),
		}
	}

	/// Request one coalesced background refresh without making a client wait for provider work.
	pub(crate) fn request_refresh(&self) {
		self.refresh_requested.notify_one();
	}

	/// Read one last daemon-owned Reset Card value without contacting the provider.
	pub(crate) async fn reset_card_inventory(
		&self,
		account_id: &AccountId,
	) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
		if self.reset_cards.is_none() {
			return Err(ResetCardServiceError::ProductStateUnavailable);
		}
		let cached = self
			.state
			.read()
			.await
			.reset_cards
			.get(account_id)
			.cloned()
			.ok_or(ResetCardServiceError::ProviderUnavailable)?;
		let current = self.accounts.inspect(account_id).await.map_err(|error| match error {
			AccountLifecycleError::AccountMissing => ResetCardServiceError::AccountNotFound,
			_ => ResetCardServiceError::ProductStateUnavailable,
		})?;
		if current.account.revision != cached.account_revision {
			return Err(ResetCardServiceError::AccountChanged);
		}
		cached.result
	}

	/// Read one persisted profile projection using only daemon-owned refresh status.
	pub(crate) async fn account_profile(
		&self,
		account_id: &AccountId,
		include_email: bool,
	) -> Option<AccountProfileRuntimeResult> {
		let profiles = self.profiles.as_ref()?;
		let status = self.state.read().await.profiles.get(account_id).copied();
		Some(profiles.read_cached(account_id, include_email, status).await)
	}

	/// Run immediate startup observation and all later coalesced daemon refreshes.
	pub(crate) async fn daemon_service(self, mut stop: watch::Receiver<bool>) {
		let reset_card_wakeup = self.reset_cards.as_ref().map(ResetCardRuntime::observation_wakeup);
		let mut interval = time::interval(OBSERVATION_REFRESH_INTERVAL);
		interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
		let mut desired = HashMap::new();
		let mut in_flight = HashMap::new();
		let mut pending = HashSet::new();
		let mut task_accounts = HashMap::new();
		let mut observations = JoinSet::new();
		loop {
			if *stop.borrow() {
				break;
			}

			tokio::select! {
				biased;

				result = stop.changed() => {
					if result.is_err() || *stop.borrow_and_update() {
						break;
					}
				},
				() = self.refresh_requested.notified() => {
					self.schedule_current_accounts(
						true,
						&mut desired,
						&mut in_flight,
						&mut pending,
						&mut task_accounts,
						&mut observations,
					).await;
				},
				() = optional_notification(reset_card_wakeup.as_ref()) => {
					self.schedule_current_accounts(
						true,
						&mut desired,
						&mut in_flight,
						&mut pending,
						&mut task_accounts,
						&mut observations,
					).await;
				},
				_ = interval.tick() => {
					self.schedule_current_accounts(
						false,
						&mut desired,
						&mut in_flight,
						&mut pending,
						&mut task_accounts,
						&mut observations,
					).await;
				},
				result = observations.join_next_with_id(), if !observations.is_empty() => {
					self.finish_observation(
						result,
						&desired,
						&mut in_flight,
						&mut pending,
						&mut task_accounts,
						&mut observations,
					).await;
				},
			}
		}

		// Provider owners retain their own bounded cleanup. Drain the async observation
		// owners so the daemon lifecycle cannot release authority while they are live.
		while observations.join_next().await.is_some() {}
	}

	async fn schedule_current_accounts(
		&self,
		queue_in_flight_successor: bool,
		desired: &mut HashMap<AccountId, i64>,
		in_flight: &mut HashMap<AccountId, TaskId>,
		pending: &mut HashSet<AccountId>,
		task_accounts: &mut HashMap<TaskId, AccountId>,
		observations: &mut JoinSet<AccountObservationOutcome>,
	) {
		let Ok((accounts, _routing)) = self.accounts.list_snapshot().await else {
			return;
		};
		let current = accounts
			.into_iter()
			.filter(|inspection| {
				!inspection.account.tombstoned
					&& inspection.readiness == AccountLifecycleReadiness::Ready
			})
			.map(|inspection| (inspection.account.account_id, inspection.account.revision))
			.collect::<HashMap<_, _>>();
		{
			let mut state = self.state.write().await;
			state.retain_current(&current);
		}
		pending.retain(|account_id| current.contains_key(account_id));
		*desired = current;

		for (account_id, account_revision) in
			plan_observation_round(desired, in_flight, pending, queue_in_flight_successor)
		{
			self.spawn_observation(
				account_id,
				account_revision,
				in_flight,
				task_accounts,
				observations,
			);
		}
	}

	fn spawn_observation(
		&self,
		account_id: AccountId,
		account_revision: i64,
		in_flight: &mut HashMap<AccountId, TaskId>,
		task_accounts: &mut HashMap<TaskId, AccountId>,
		observations: &mut JoinSet<AccountObservationOutcome>,
	) {
		let profiles = self.profiles.clone();
		let reset_cards = self.reset_cards.clone();
		let task_account_id = account_id.clone();
		let task = observations.spawn(async move {
			// Reset Card observation can rotate the account credential. Complete it first so the
			// profile observation uses that exact successor instead of racing an old revision.
			let reset_cards = match reset_cards {
				Some(reset_cards) => Some(reset_cards.observe_inventory(&task_account_id).await),
				None => None,
			};
			let profile_revision = reset_cards
				.as_ref()
				.and_then(|result| result.as_ref().ok())
				.map_or(account_revision, inventory_revision);
			let profile = match profiles {
				Some(profiles) => Some(
					profiles.query(&task_account_id, false).await.refresh_status(profile_revision),
				),
				None => None,
			};
			AccountObservationOutcome {
				account_id: task_account_id,
				requested_revision: account_revision,
				reset_cards,
				profile,
			}
		});
		let task_id = task.id();
		in_flight.insert(account_id.clone(), task_id);
		task_accounts.insert(task_id, account_id);
	}

	async fn finish_observation(
		&self,
		result: Option<Result<(TaskId, AccountObservationOutcome), tokio::task::JoinError>>,
		desired: &HashMap<AccountId, i64>,
		in_flight: &mut HashMap<AccountId, TaskId>,
		pending: &mut HashSet<AccountId>,
		task_accounts: &mut HashMap<TaskId, AccountId>,
		observations: &mut JoinSet<AccountObservationOutcome>,
	) {
		let (task_id, observation) = match result {
			Some(Ok((task_id, observation))) => (task_id, Some(observation)),
			Some(Err(error)) => (error.id(), None),
			None => return,
		};
		let Some(account_id) = task_accounts.remove(&task_id) else {
			return;
		};
		if in_flight.get(&account_id) == Some(&task_id) {
			in_flight.remove(&account_id);
		}

		if let Some(observation) = observation
			&& observation.account_id == account_id
			&& desired.get(&account_id) == Some(&observation.requested_revision)
		{
			let mut state = self.state.write().await;
			state.insert(observation);
		}

		if pending.remove(&account_id)
			&& let Some(account_revision) = desired.get(&account_id)
		{
			self.spawn_observation(
				account_id,
				*account_revision,
				in_flight,
				task_accounts,
				observations,
			);
		}
	}
}

async fn optional_notification(notify: Option<&Arc<Notify>>) {
	match notify {
		Some(notify) => notify.notified().await,
		None => future::pending().await,
	}
}

fn plan_observation_round<T>(
	desired: &HashMap<AccountId, i64>,
	in_flight: &HashMap<AccountId, T>,
	pending: &mut HashSet<AccountId>,
	queue_in_flight_successor: bool,
) -> Vec<(AccountId, i64)> {
	desired
		.iter()
		.filter_map(|(account_id, account_revision)| {
			if in_flight.contains_key(account_id) {
				if queue_in_flight_successor {
					pending.insert(account_id.clone());
				}
				None
			} else {
				Some((account_id.clone(), *account_revision))
			}
		})
		.collect()
}

const fn inventory_revision(observation: &ResetCardInventoryObservation) -> i64 {
	match observation {
		ResetCardInventoryObservation::Available(inventory) => inventory.account_revision,
		ResetCardInventoryObservation::ObservationFailed(failure) => failure.account_revision,
	}
}

#[cfg(test)]
mod tests {
	use std::collections::{HashMap, HashSet};

	use decodex_core::AccountId;

	use super::{
		AccountObservationOutcome, AccountObservationState, AccountProfileRefreshStatus,
		ResetCardServiceError, plan_observation_round,
	};

	#[test]
	fn one_round_starts_every_independent_account_without_a_global_cap() {
		let desired =
			(1..=12).map(|index| (account(index), i64::from(index))).collect::<HashMap<_, _>>();
		let active = account(4);
		let in_flight = HashMap::from([(active.clone(), ())]);
		let mut pending = HashSet::new();

		let scheduled = plan_observation_round(&desired, &in_flight, &mut pending, true);

		assert_eq!(scheduled.len(), 11);
		assert!(scheduled.iter().all(|(account_id, _)| account_id != &active));
		assert_eq!(pending, HashSet::from([active]));
	}

	#[test]
	fn periodic_round_does_not_hot_loop_a_slow_account() {
		let active = account(1);
		let desired = HashMap::from([(active.clone(), 7)]);
		let in_flight = HashMap::from([(active, ())]);
		let mut pending = HashSet::new();

		let scheduled = plan_observation_round(&desired, &in_flight, &mut pending, false);

		assert!(scheduled.is_empty());
		assert!(pending.is_empty());
	}

	#[test]
	fn revision_change_prunes_both_daemon_owned_account_values() {
		let account_id = account(1);
		let mut state = AccountObservationState::default();
		state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 7,
			reset_cards: Some(Err(ResetCardServiceError::ProviderUnavailable)),
			profile: Some(AccountProfileRefreshStatus { account_revision: 7, refresh_error: None }),
		});

		state.retain_current(&HashMap::from([(account_id, 8)]));

		assert!(state.reset_cards.is_empty());
		assert!(state.profiles.is_empty());
	}

	#[test]
	fn profile_observation_does_not_require_reset_card_capability() {
		let account_id = account(2);
		let mut state = AccountObservationState::default();
		state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 9,
			reset_cards: None,
			profile: Some(AccountProfileRefreshStatus { account_revision: 9, refresh_error: None }),
		});

		assert!(state.reset_cards.is_empty());
		assert_eq!(
			state.profiles.get(&account_id),
			Some(&AccountProfileRefreshStatus { account_revision: 9, refresh_error: None })
		);
	}

	fn account(index: u16) -> AccountId {
		AccountId::new(format!("00000000-0000-4000-8000-{index:012}"))
			.expect("fixture account ID is canonical")
	}
}
