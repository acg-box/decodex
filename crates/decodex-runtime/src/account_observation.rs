//! Daemon-owned background observation for every independent account.

use std::{
	collections::{HashMap, HashSet},
	future,
	sync::Arc,
	time::Duration,
};

use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountQuotaDisposition, AccountQuotaWindowObservation,
};
use decodex_protocol::AccountObservationSignal;
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
	account_service::AccountService,
};

const OBSERVATION_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const OBSERVATION_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct CachedResetCardInventory {
	account_revision: i64,
	result: Result<ResetCardInventoryObservation, ResetCardServiceError>,
}

#[derive(Default)]
struct AccountObservationState {
	reset_cards: HashMap<AccountId, CachedResetCardInventory>,
	profiles: HashMap<AccountId, AccountProfileRefreshStatus>,
	cache_generations: HashMap<AccountId, Arc<()>>,
}

struct AccountObservationOutcome {
	account_id: AccountId,
	requested_revision: i64,
	cache_generation: Arc<()>,
	reset_cards: Option<Result<ResetCardInventoryObservation, ResetCardServiceError>>,
	profile: Option<AccountProfileRefreshStatus>,
}

impl AccountObservationState {
	fn retain_current(&mut self, current: &HashMap<AccountId, i64>) -> bool {
		let prior_reset_cards = self.reset_cards.len();
		let prior_profiles = self.profiles.len();
		let prior_generations = self.cache_generations.len();
		self.reset_cards.retain(|account_id, cached| {
			current.get(account_id).is_some_and(|revision| *revision == cached.account_revision)
		});
		self.profiles.retain(|account_id, status| {
			current.get(account_id).is_some_and(|revision| *revision == status.account_revision)
		});
		self.cache_generations.retain(|account_id, _| current.contains_key(account_id));
		prior_reset_cards != self.reset_cards.len()
			|| prior_profiles != self.profiles.len()
			|| prior_generations != self.cache_generations.len()
	}

	fn invalidate_account(&mut self, account_id: &AccountId) {
		self.cache_generations.insert(account_id.clone(), Arc::new(()));
		self.reset_cards.remove(account_id);
		self.profiles.remove(account_id);
	}

	fn cache_generation(&mut self, account_id: &AccountId) -> Arc<()> {
		Arc::clone(self.cache_generations.entry(account_id.clone()).or_insert_with(|| Arc::new(())))
	}

	fn generation_matches(&self, account_id: &AccountId, generation: &Arc<()>) -> bool {
		self.cache_generations
			.get(account_id)
			.is_some_and(|current| Arc::ptr_eq(current, generation))
	}

	fn reset_card_inventory(
		&self,
		account_id: &AccountId,
	) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
		self.reset_cards
			.get(account_id)
			.cloned()
			.ok_or(ResetCardServiceError::ProviderUnavailable)?
			.result
	}

	fn insert(&mut self, observation: AccountObservationOutcome) -> bool {
		if !self.generation_matches(&observation.account_id, &observation.cache_generation) {
			return false;
		}
		let account_id = observation.account_id.clone();
		let reset_cards_changed = observation.reset_cards.as_ref().is_some_and(|next| {
			self.reset_cards.get(&account_id).is_none_or(|current| {
				!reset_card_observation_semantically_equal(&current.result, next)
			})
		});
		let profile_changed = observation.profile.as_ref().is_some_and(|next| {
			self.profiles
				.get(&account_id)
				.is_none_or(|current| !profile_status_semantically_equal(current, next))
		});
		if let Some(reset_cards) = observation.reset_cards {
			let inventory_revision = reset_cards
				.as_ref()
				.ok()
				.map(inventory_revision)
				.unwrap_or(observation.requested_revision);
			self.reset_cards.insert(
				account_id.clone(),
				CachedResetCardInventory {
					account_revision: inventory_revision,
					result: reset_cards,
				},
			);
		}
		if let Some(profile) = observation.profile {
			self.profiles.insert(account_id, profile);
		}
		reset_cards_changed || profile_changed
	}
}

fn reset_card_observation_semantically_equal(
	left: &Result<ResetCardInventoryObservation, ResetCardServiceError>,
	right: &Result<ResetCardInventoryObservation, ResetCardServiceError>,
) -> bool {
	match (left, right) {
		(
			Ok(ResetCardInventoryObservation::Available(left)),
			Ok(ResetCardInventoryObservation::Available(right)),
		) =>
			left.account_id == right.account_id
				&& left.account_revision == right.account_revision
				&& left.reported_available_count == right.reported_available_count
				&& left.details_complete == right.details_complete
				&& left.cards == right.cards
				&& quota_observation_semantically_equal(
					&left.five_hour_quota,
					&right.five_hour_quota,
				) && quota_observation_semantically_equal(&left.seven_day_quota, &right.seven_day_quota),
		(
			Ok(ResetCardInventoryObservation::ObservationFailed(left)),
			Ok(ResetCardInventoryObservation::ObservationFailed(right)),
		) =>
			left.account_id == right.account_id
				&& left.account_revision == right.account_revision
				&& quota_observation_semantically_equal(
					&left.five_hour_quota,
					&right.five_hour_quota,
				) && quota_observation_semantically_equal(&left.seven_day_quota, &right.seven_day_quota)
				&& left.error == right.error,
		(Err(left), Err(right)) => left == right,
		_ => false,
	}
}

fn quota_observation_semantically_equal(
	left: &AccountQuotaWindowObservation,
	right: &AccountQuotaWindowObservation,
) -> bool {
	left.duration_minutes == right.duration_minutes
		&& match (left.disposition, right.disposition) {
			(AccountQuotaDisposition::Unknown, AccountQuotaDisposition::Unknown) => true,
			(AccountQuotaDisposition::Current(left), AccountQuotaDisposition::Current(right)) =>
				left == right,
			(AccountQuotaDisposition::Stale(left), AccountQuotaDisposition::Stale(right)) =>
				left == right,
			(AccountQuotaDisposition::Error(left), AccountQuotaDisposition::Error(right)) =>
				left == right,
			_ => false,
		}
}

fn profile_status_semantically_equal(
	left: &AccountProfileRefreshStatus,
	right: &AccountProfileRefreshStatus,
) -> bool {
	left.account_revision == right.account_revision
		&& left.refresh_error == right.refresh_error
		&& match (&left.snapshot, &right.snapshot) {
			(None, None) => true,
			(Some(left), Some(right)) =>
				left.account_id == right.account_id
					&& left.account_revision == right.account_revision
					&& left.provider == right.provider
					&& left.display_name == right.display_name
					&& left.username == right.username
					&& left.lifetime_tokens == right.lifetime_tokens
					&& left.peak_daily_tokens == right.peak_daily_tokens
					&& left.longest_task_seconds == right.longest_task_seconds
					&& left.current_streak_days == right.current_streak_days
					&& left.longest_streak_days == right.longest_streak_days
					&& left.daily_usage == right.daily_usage,
			_ => false,
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
	observation_generation: watch::Sender<u64>,
}

impl AccountObservationService {
	pub(crate) fn new(
		accounts: Arc<AccountService>,
		profiles: Option<AccountProfileRuntime>,
		reset_cards: Option<ResetCardRuntime>,
	) -> Self {
		let (observation_generation, _) = watch::channel(0);
		Self {
			accounts,
			profiles,
			reset_cards,
			state: Arc::new(RwLock::new(AccountObservationState::default())),
			refresh_requested: Arc::new(Notify::new()),
			observation_generation,
		}
	}

	/// Request one coalesced background refresh without making a client wait for provider work.
	pub(crate) fn request_refresh(&self) {
		self.refresh_requested.notify_one();
	}

	/// Invalidate one account before a later in-flight observation can publish its value.
	pub(crate) async fn invalidate_account(&self, account_id: &AccountId) {
		self.state.write().await.invalidate_account(account_id);
		self.advance_observation_generation();
	}

	/// Wait for daemon-owned values to advance, with one bounded heartbeat fallback.
	pub(crate) async fn wait_for_change(&self, after_generation: u64) -> AccountObservationSignal {
		wait_for_generation(
			self.observation_generation.subscribe(),
			after_generation,
			OBSERVATION_WAIT_TIMEOUT,
		)
		.await
	}

	/// Delay an unavailable observer response by the same bounded heartbeat window.
	pub(crate) async fn heartbeat(generation: u64) -> AccountObservationSignal {
		time::sleep(OBSERVATION_WAIT_TIMEOUT).await;
		AccountObservationSignal::new(generation)
	}

	/// Read one last daemon-owned Reset Card value without contacting the provider.
	pub(crate) async fn reset_card_inventory(
		&self,
		account_id: &AccountId,
	) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
		if self.reset_cards.is_none() {
			return Err(ResetCardServiceError::ProductStateUnavailable);
		}
		self.state.read().await.reset_card_inventory(account_id)
	}

	/// Read one persisted profile projection using only daemon-owned refresh status.
	pub(crate) async fn account_profile(
		&self,
		account_id: &AccountId,
		include_email: bool,
	) -> Option<AccountProfileRuntimeResult> {
		let profiles = self.profiles.as_ref()?;
		let status = self.state.read().await.profiles.get(account_id).cloned();
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
		let cache_pruned = {
			let mut state = self.state.write().await;
			state.retain_current(&current)
		};
		if cache_pruned {
			self.advance_observation_generation();
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
			)
			.await;
		}
	}

	async fn spawn_observation(
		&self,
		account_id: AccountId,
		account_revision: i64,
		in_flight: &mut HashMap<AccountId, TaskId>,
		task_accounts: &mut HashMap<TaskId, AccountId>,
		observations: &mut JoinSet<AccountObservationOutcome>,
	) {
		let cache_generation = self.state.write().await.cache_generation(&account_id);
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
				cache_generation,
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
			let published = {
				let mut state = self.state.write().await;
				state.insert(observation)
			};
			if published {
				self.advance_observation_generation();
			}
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
			)
			.await;
		}
	}

	fn advance_observation_generation(&self) {
		self.observation_generation.send_modify(|generation| {
			*generation = generation.wrapping_add(1);
		});
	}
}

async fn wait_for_generation(
	mut changes: watch::Receiver<u64>,
	after_generation: u64,
	wait_timeout: Duration,
) -> AccountObservationSignal {
	let current = *changes.borrow_and_update();
	if current != after_generation {
		return AccountObservationSignal::new(current);
	}

	let _ = time::timeout(wait_timeout, async {
		loop {
			if changes.changed().await.is_err() {
				return;
			}
			if *changes.borrow_and_update() != after_generation {
				return;
			}
		}
	})
	.await;
	let generation = *changes.borrow_and_update();
	AccountObservationSignal::new(generation)
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
	use std::{
		collections::{HashMap, HashSet},
		sync::Arc,
		time::Duration,
	};

	use decodex_core::{
		AccountId, AccountProvider, AccountQuotaWindowObservation, ProviderIdentity,
	};
	use decodex_postgres::AccountProfileSnapshot;
	use tokio::{sync::watch, time};

	use crate::account_launch::ResetCardInventoryView;

	use super::{
		AccountObservationOutcome, AccountObservationState, AccountProfileRefreshStatus,
		ResetCardInventoryObservation, ResetCardServiceError, plan_observation_round,
		wait_for_generation,
	};

	#[tokio::test]
	async fn observation_signal_is_immediate_on_change_and_bounded_when_unchanged() {
		let (generation, initial) = watch::channel(7_u64);
		let immediate = wait_for_generation(initial, 6, Duration::from_secs(1)).await;
		assert_eq!(immediate.generation, 7);

		let waiting = wait_for_generation(generation.subscribe(), 7, Duration::from_secs(1));
		tokio::pin!(waiting);
		assert!(time::timeout(Duration::from_millis(5), &mut waiting).await.is_err());
		generation.send_modify(|value| *value = 8);
		assert_eq!(waiting.await.generation, 8);

		let heartbeat = wait_for_generation(generation.subscribe(), 8, Duration::ZERO).await;
		assert_eq!(heartbeat.generation, 8);
	}

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
		let cache_generation = state.cache_generation(&account_id);
		state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 7,
			cache_generation,
			reset_cards: Some(Err(ResetCardServiceError::InventoryIncomplete)),
			profile: Some(AccountProfileRefreshStatus {
				account_revision: 7,
				refresh_error: None,
				snapshot: None,
			}),
		});

		state.retain_current(&HashMap::from([(account_id.clone(), 8)]));

		assert!(state.reset_cards.is_empty());
		assert!(state.profiles.is_empty());
		assert_eq!(
			state.reset_card_inventory(&account_id),
			Err(ResetCardServiceError::ProviderUnavailable)
		);
	}

	#[test]
	fn invalidation_fences_a_late_in_flight_observation_without_a_database_read() {
		let account_id = account(2);
		let mut state = AccountObservationState::default();

		let stale_generation = state.cache_generation(&account_id);
		state.invalidate_account(&account_id);
		state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 9,
			cache_generation: stale_generation,
			reset_cards: Some(Err(ResetCardServiceError::InventoryIncomplete)),
			profile: Some(AccountProfileRefreshStatus {
				account_revision: 9,
				refresh_error: None,
				snapshot: None,
			}),
		});

		assert!(state.reset_cards.is_empty());
		assert!(state.profiles.is_empty());
		assert_eq!(
			state.reset_card_inventory(&account_id),
			Err(ResetCardServiceError::ProviderUnavailable)
		);

		let cache_generation = state.cache_generation(&account_id);
		state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 9,
			cache_generation,
			reset_cards: Some(Err(ResetCardServiceError::InventoryIncomplete)),
			profile: Some(AccountProfileRefreshStatus {
				account_revision: 9,
				refresh_error: None,
				snapshot: None,
			}),
		});

		assert!(state.reset_cards.contains_key(&account_id));
		assert!(state.profiles.contains_key(&account_id));
		assert_eq!(
			state.reset_card_inventory(&account_id),
			Err(ResetCardServiceError::InventoryIncomplete)
		);
	}

	#[test]
	fn profile_observation_does_not_require_reset_card_capability() {
		let account_id = account(2);
		let mut state = AccountObservationState::default();
		let cache_generation = state.cache_generation(&account_id);
		state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 9,
			cache_generation,
			reset_cards: None,
			profile: Some(AccountProfileRefreshStatus {
				account_revision: 9,
				refresh_error: None,
				snapshot: None,
			}),
		});

		assert!(state.reset_cards.is_empty());
		assert_eq!(
			state.profiles.get(&account_id),
			Some(&AccountProfileRefreshStatus {
				account_revision: 9,
				refresh_error: None,
				snapshot: None,
			})
		);
	}

	#[test]
	fn unchanged_observation_updates_freshness_without_advancing_generation() {
		let account_id = account(3);
		let mut state = AccountObservationState::default();
		let cache_generation = state.cache_generation(&account_id);

		assert!(state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation: Arc::clone(&cache_generation),
			reset_cards: Some(Ok(available_inventory(&account_id, 100))),
			profile: None,
		}));

		assert!(!state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation,
			reset_cards: Some(Ok(available_inventory(&account_id, 200))),
			profile: None,
		}));

		let cached = state.reset_cards.get(&account_id).expect("cached observation");
		let Ok(ResetCardInventoryObservation::Available(inventory)) = &cached.result else {
			panic!("expected available inventory");
		};
		assert_eq!(inventory.five_hour_quota.observed_at_unix_micros, Some(200));
		assert_eq!(inventory.seven_day_quota.observed_at_unix_micros, Some(200));
	}

	#[test]
	fn unchanged_profile_updates_observation_time_without_advancing_generation() {
		let account_id = account(4);
		let mut state = AccountObservationState::default();
		let cache_generation = state.cache_generation(&account_id);

		assert!(state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 12,
			cache_generation: Arc::clone(&cache_generation),
			reset_cards: None,
			profile: Some(profile_status(&account_id, 100)),
		}));

		assert!(!state.insert(AccountObservationOutcome {
			account_id: account_id.clone(),
			requested_revision: 12,
			cache_generation,
			reset_cards: None,
			profile: Some(profile_status(&account_id, 200)),
		}));

		assert_eq!(
			state
				.profiles
				.get(&account_id)
				.and_then(|status| status.snapshot.as_ref())
				.map(|snapshot| snapshot.observed_at_unix_micros),
			Some(200)
		);
	}

	fn available_inventory(
		account_id: &AccountId,
		observed_at_unix_micros: i64,
	) -> ResetCardInventoryObservation {
		ResetCardInventoryObservation::Available(ResetCardInventoryView {
			account_id: account_id.clone(),
			account_revision: 11,
			reported_available_count: Some(0),
			details_complete: true,
			cards: Vec::new(),
			five_hour_quota: quota(300, observed_at_unix_micros, 20, 2_000_000),
			seven_day_quota: quota(10_080, observed_at_unix_micros, 30, 3_000_000),
		})
	}

	fn profile_status(
		account_id: &AccountId,
		observed_at_unix_micros: i64,
	) -> AccountProfileRefreshStatus {
		AccountProfileRefreshStatus {
			account_revision: 12,
			refresh_error: None,
			snapshot: Some(AccountProfileSnapshot {
				account_id: account_id.clone(),
				account_revision: 12,
				provider: ProviderIdentity::new(AccountProvider::Chatgpt, "provider-account")
					.expect("fixture provider identity"),
				observed_at_unix_micros,
				display_name: None,
				username: None,
				lifetime_tokens: None,
				peak_daily_tokens: None,
				longest_task_seconds: None,
				current_streak_days: None,
				longest_streak_days: None,
				daily_usage: Vec::new(),
			}),
		}
	}

	fn quota(
		duration_minutes: u32,
		observed_at_unix_micros: i64,
		used_percent: u8,
		resets_at_unix_micros: i64,
	) -> AccountQuotaWindowObservation {
		AccountQuotaWindowObservation {
			duration_minutes,
			observed_at_unix_micros: Some(observed_at_unix_micros),
			disposition: decodex_core::AccountQuotaDisposition::Current(
				decodex_core::AccountQuotaWindow {
					duration_minutes,
					used_percent,
					resets_at_unix_micros,
				},
			),
		}
	}

	fn account(index: u16) -> AccountId {
		AccountId::new(format!("00000000-0000-4000-8000-{index:012}"))
			.expect("fixture account ID is canonical")
	}
}
