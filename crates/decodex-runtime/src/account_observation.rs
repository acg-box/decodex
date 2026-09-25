//! Daemon-owned background observation for every independent account.

mod recovery;
mod recovery_actions;
use recovery::CachedAccountBanner;

use std::{
	collections::{HashMap, HashSet},
	future,
	sync::Arc,
	time::Duration,
};

use decodex_core::{
	AccountId, AccountQuotaDisposition, AccountQuotaObservationError, AccountQuotaWindow,
	AccountQuotaWindowObservation,
};
use decodex_protocol::AccountObservationSignal;
use tokio::{
	sync::{Notify, RwLock, watch},
	task::{Id as TaskId, JoinSet},
	time::{self, MissedTickBehavior},
};

use crate::{
	account_api::{
		AccountApiInventory, AccountApiObservation, AccountApiRuntime, AccountApiRuntimeError,
	},
	account_launch::{ApiResetCardRuntime, ResetCardInventoryObservation, ResetCardServiceError},
	account_profile::{
		AccountProfileRefreshStatus, AccountProfileRuntime, AccountProfileRuntimeError,
		AccountProfileRuntimeResult,
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

async fn direct_inventory_observation(
	accounts: &AccountService,
	account_id: &AccountId,
	requested_revision: i64,
	observation: &AccountApiObservation,
) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
	let inventory = match &observation.inventory {
		Ok(inventory) => inventory,
		Err(error) => {
			let [five_hour_quota, seven_day_quota] =
				cached_direct_quotas(accounts, account_id, map_api_error_to_quota(*error)).await;
			return Ok(ResetCardInventoryObservation::ObservationFailed(
				crate::account_launch::ResetCardObservationFailure {
					account_id: account_id.clone(),
					account_revision: requested_revision,
					five_hour_quota,
					seven_day_quota,
					error: map_api_error_to_reset(*error),
				},
			));
		},
	};
	let [five_hour_quota, seven_day_quota] =
		persist_direct_quotas(accounts, account_id, inventory).await?;
	Ok(ResetCardInventoryObservation::Available(crate::account_launch::ResetCardInventoryView {
		account_id: account_id.clone(),
		account_revision: inventory.account_revision,
		reported_available_count: inventory.reported_available_count,
		details_complete: inventory.details_complete,
		cards: if inventory.details_complete {
			inventory.credits.iter().map(|credit| credit.descriptor()).collect()
		} else {
			Vec::new()
		},
		five_hour_quota,
		seven_day_quota,
	}))
}

async fn persist_direct_quotas(
	accounts: &AccountService,
	account_id: &AccountId,
	inventory: &AccountApiInventory,
) -> Result<[AccountQuotaWindowObservation; 2], ResetCardServiceError> {
	let inspection = accounts
		.inspect(account_id)
		.await
		.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?;
	if inspection.account.revision != inventory.account_revision {
		return Err(ResetCardServiceError::ProductStateUnavailable);
	}
	let cached = inspection
		.account
		.usage_observation
		.filter(|observation| observation.account_revision == inventory.account_revision)
		.map(|_| [inspection.account.five_hour_quota, inspection.account.seven_day_quota]);
	let mut accepted = [None, None];
	let mut observations = Vec::with_capacity(2);
	let now = current_unix_micros().ok_or(ResetCardServiceError::ProductStateUnavailable)?;
	let optional_five_absent = has_optional_five_absence(&inventory.quota_windows, now);
	for (index, quota) in inventory.quota_windows.into_iter().enumerate() {
		let observed_at_unix_micros = now;
		let cached_window = cached.as_ref().and_then(|windows| {
			windows.iter().find(|window| window.duration_minutes == quota.duration_minutes).copied()
		});
		let (observed_at_unix_micros, disposition) = match quota.result {
			Ok(None)
				if optional_five_absent
					&& quota.duration_minutes == AccountQuotaWindow::FIVE_HOURS_MINUTES =>
				(Some(observed_at_unix_micros), AccountQuotaDisposition::NotApplicable),
			Ok(Some(fact)) => (
				Some(observed_at_unix_micros),
				if fact.resets_at_unix_micros > now {
					AccountQuotaDisposition::Current(fact)
				} else {
					AccountQuotaDisposition::Stale(fact)
				},
			),
			result => resolve_direct_quota(
				quota.duration_minutes,
				result,
				cached_window,
				observed_at_unix_micros,
			),
		};
		if matches!(quota.result, Ok(Some(fact)) if fact.resets_at_unix_micros > now)
			|| (quota.result == Ok(None) && optional_five_absent && index == 0)
		{
			accepted[index] = Some(AccountQuotaWindowObservation {
				duration_minutes: quota.duration_minutes,
				observed_at_unix_micros,
				disposition,
			});
		}
		observations.push(AccountQuotaWindowObservation {
			duration_minutes: quota.duration_minutes,
			observed_at_unix_micros,
			disposition,
		});
	}
	if !accounts
		.observe_usage(
			account_id,
			decodex_core::AccountUsageObservation {
				account_revision: inventory.account_revision,
				observed_at_unix_micros: now,
				ordinary_usage_allowed: inventory.ordinary_usage_allowed,
				conditions: inventory.conditions,
			},
			accepted,
		)
		.await
		.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?
	{
		return Err(ResetCardServiceError::ProductStateUnavailable);
	}
	observations.try_into().map_err(|_| ResetCardServiceError::InventoryIncomplete)
}

fn has_optional_five_absence(
	windows: &[decodex_codex::AccountApiQuotaWindow; 2],
	now: i64,
) -> bool {
	windows.iter().any(|window|window.duration_minutes==AccountQuotaWindow::FIVE_HOURS_MINUTES && matches!(window.result,Ok(None)))
		&& windows.iter().any(|window|window.duration_minutes==AccountQuotaWindow::SEVEN_DAYS_MINUTES
			&& matches!(window.result,Ok(Some(fact)) if fact.duration_minutes==AccountQuotaWindow::SEVEN_DAYS_MINUTES && fact.resets_at_unix_micros>now))
}

fn resolve_direct_quota(
	duration_minutes: u32,
	result: Result<Option<AccountQuotaWindow>, AccountQuotaObservationError>,
	cached: Option<AccountQuotaWindowObservation>,
	observed_at_unix_micros: i64,
) -> (Option<i64>, AccountQuotaDisposition) {
	match result {
		Ok(None) => retained_last_good_quota(cached, duration_minutes)
			.unwrap_or((None, AccountQuotaDisposition::Unknown)),
		Err(error) => retained_last_good_quota(cached, duration_minutes)
			.unwrap_or((Some(observed_at_unix_micros), AccountQuotaDisposition::Error(error))),
		Ok(Some(fact)) => (Some(observed_at_unix_micros), AccountQuotaDisposition::Current(fact)),
	}
}

fn retained_last_good_quota(
	cached: Option<AccountQuotaWindowObservation>,
	duration_minutes: u32,
) -> Option<(Option<i64>, AccountQuotaDisposition)> {
	let window = cached.filter(|window| window.duration_minutes == duration_minutes)?;
	match window.disposition {
		AccountQuotaDisposition::Current(_)
		| AccountQuotaDisposition::Stale(_)
		| AccountQuotaDisposition::NotApplicable =>
			Some((window.observed_at_unix_micros, window.disposition)),
		AccountQuotaDisposition::Unknown | AccountQuotaDisposition::Error(_) => None,
	}
}

async fn cached_direct_quotas(
	accounts: &AccountService,
	account_id: &AccountId,
	error: AccountQuotaObservationError,
) -> [AccountQuotaWindowObservation; 2] {
	let cached =
		accounts.inspect(account_id).await.ok().map(|inspection| {
			[inspection.account.five_hour_quota, inspection.account.seven_day_quota]
		});
	[
		cached
			.as_ref()
			.and_then(|windows| {
				windows
					.iter()
					.find(|window| {
						window.duration_minutes == AccountQuotaWindow::FIVE_HOURS_MINUTES
					})
					.copied()
			})
			.and_then(|window| {
				matches!(
					window.disposition,
					AccountQuotaDisposition::Current(_)
						| AccountQuotaDisposition::Stale(_)
						| AccountQuotaDisposition::NotApplicable
				)
				.then_some(window)
			})
			.unwrap_or_else(|| {
				quota_error_observation(AccountQuotaWindow::FIVE_HOURS_MINUTES, error)
			}),
		cached
			.as_ref()
			.and_then(|windows| {
				windows
					.iter()
					.find(|window| {
						window.duration_minutes == AccountQuotaWindow::SEVEN_DAYS_MINUTES
					})
					.copied()
			})
			.and_then(|window| {
				matches!(
					window.disposition,
					AccountQuotaDisposition::Current(_)
						| AccountQuotaDisposition::Stale(_)
						| AccountQuotaDisposition::NotApplicable
				)
				.then_some(window)
			})
			.unwrap_or_else(|| {
				quota_error_observation(AccountQuotaWindow::SEVEN_DAYS_MINUTES, error)
			}),
	]
}

fn quota_error_observation(
	duration_minutes: u32,
	error: AccountQuotaObservationError,
) -> AccountQuotaWindowObservation {
	AccountQuotaWindowObservation {
		duration_minutes,
		observed_at_unix_micros: current_unix_micros(),
		disposition: AccountQuotaDisposition::Error(error),
	}
}

fn current_unix_micros() -> Option<i64> {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
}

fn map_api_error_to_quota(error: AccountApiRuntimeError) -> AccountQuotaObservationError {
	match error {
		AccountApiRuntimeError::AccountChanged | AccountApiRuntimeError::AccountUnavailable =>
			AccountQuotaObservationError::AccountMismatch,
		AccountApiRuntimeError::ProtocolUnavailable =>
			AccountQuotaObservationError::ProtocolUnavailable,
		AccountApiRuntimeError::CredentialUnavailable
		| AccountApiRuntimeError::CredentialBusy
		| AccountApiRuntimeError::RefreshRejected
		| AccountApiRuntimeError::RefreshAmbiguous
		| AccountApiRuntimeError::AccessRejectedAfterRefresh
		| AccountApiRuntimeError::Unauthorized
		| AccountApiRuntimeError::ProviderUnavailable =>
			AccountQuotaObservationError::ProviderUnavailable,
	}
}

fn map_api_error_to_reset(error: AccountApiRuntimeError) -> ResetCardServiceError {
	match error {
		AccountApiRuntimeError::AccountUnavailable => ResetCardServiceError::AccountNotFound,
		AccountApiRuntimeError::CredentialUnavailable => ResetCardServiceError::VaultUnavailable,
		AccountApiRuntimeError::CredentialBusy => ResetCardServiceError::ProductStateUnavailable,
		AccountApiRuntimeError::RefreshRejected => ResetCardServiceError::AccountStateRejected,
		AccountApiRuntimeError::RefreshAmbiguous => ResetCardServiceError::VaultUnavailable,
		AccountApiRuntimeError::AccessRejectedAfterRefresh =>
			ResetCardServiceError::AccountStateRejected,
		AccountApiRuntimeError::AccountChanged => ResetCardServiceError::AccountChanged,
		AccountApiRuntimeError::ProtocolUnavailable => ResetCardServiceError::InventoryIncomplete,
		AccountApiRuntimeError::Unauthorized | AccountApiRuntimeError::ProviderUnavailable =>
			ResetCardServiceError::ProviderUnavailable,
	}
}

fn map_profile_api_error(error: AccountApiRuntimeError) -> AccountProfileRuntimeError {
	match error {
		AccountApiRuntimeError::AccountUnavailable =>
			AccountProfileRuntimeError::AccountUnavailable,
		AccountApiRuntimeError::CredentialUnavailable =>
			AccountProfileRuntimeError::CredentialUnavailable,
		AccountApiRuntimeError::CredentialBusy => AccountProfileRuntimeError::CredentialBusy,
		AccountApiRuntimeError::RefreshRejected => AccountProfileRuntimeError::RefreshRejected,
		AccountApiRuntimeError::RefreshAmbiguous => AccountProfileRuntimeError::RefreshAmbiguous,
		AccountApiRuntimeError::AccessRejectedAfterRefresh =>
			AccountProfileRuntimeError::AccessRejectedAfterRefresh,
		AccountApiRuntimeError::Unauthorized => AccountProfileRuntimeError::Unauthorized,
		AccountApiRuntimeError::AccountChanged => AccountProfileRuntimeError::AccountChanged,
		AccountApiRuntimeError::ProtocolUnavailable =>
			AccountProfileRuntimeError::ProtocolUnavailable,
		AccountApiRuntimeError::ProviderUnavailable =>
			AccountProfileRuntimeError::ProviderUnavailable,
	}
}

#[derive(Default)]
struct AccountObservationState {
	banners: HashMap<AccountId, CachedAccountBanner>,
	reset_cards: HashMap<AccountId, CachedResetCardInventory>,
	profiles: HashMap<AccountId, AccountProfileRefreshStatus>,
	cache_generations: HashMap<AccountId, Arc<()>>,
}

struct AccountObservationOutcome {
	banner: Option<CachedAccountBanner>,
	account_id: AccountId,
	requested_revision: i64,
	cache_generation: Arc<()>,
	reset_cards: Option<Result<ResetCardInventoryObservation, ResetCardServiceError>>,
	profile: Option<AccountProfileRefreshStatus>,
}

impl AccountObservationState {
	fn retain_current(&mut self, current: &HashMap<AccountId, i64>) -> bool {
		let prior_banners = self.banners.len();
		self.banners.retain(|id, banner| current.get(id) == Some(&banner.account_revision));
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
		prior_banners != self.banners.len()
			|| prior_reset_cards != self.reset_cards.len()
			|| prior_profiles != self.profiles.len()
			|| prior_generations != self.cache_generations.len()
	}

	fn invalidate_account(&mut self, account_id: &AccountId) {
		self.cache_generations.insert(account_id.clone(), Arc::new(()));
		self.banners.remove(account_id);
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
		let banner_changed = if let Some(next) = observation.banner.clone() {
			let changed = self.banners.get(&observation.account_id).is_none_or(|old| {
				old.account_revision != next.account_revision
					|| old.current != next.current
					|| old.banner != next.banner
					|| old.context != next.context
			});
			self.banners.insert(observation.account_id.clone(), next);
			changed
		} else if let Some(old) = self.banners.get_mut(&observation.account_id) {
			std::mem::replace(&mut old.current, false)
		} else {
			false
		};
		let mut observation = observation;
		if let Some(next) = observation.reset_cards.take() {
			observation.reset_cards = Some(
				self.reset_cards
					.get(&observation.account_id)
					.map(|current| {
						retain_last_good_inventory(
							current,
							observation.requested_revision,
							next.clone(),
						)
					})
					.unwrap_or(next),
			);
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
		banner_changed || reset_cards_changed || profile_changed
	}
}

#[cfg(test)]
mod recovery_cache_tests {
	use super::{
		AccountId, AccountObservationOutcome, AccountObservationState, CachedAccountBanner,
	};
	use decodex_codex::AccountApiBannerState;
	use std::{collections::HashMap, sync::Arc};
	#[test]
	fn invalidated_generation_cannot_restore_recovery_and_failures_expire_it() {
		let account = AccountId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let mut state = AccountObservationState::default();
		let generation = state.cache_generation(&account);
		let outcome = |generation, banner| AccountObservationOutcome {
			account_id: account.clone(),
			requested_revision: 1,
			cache_generation: generation,
			banner,
			reset_cards: None,
			profile: None,
		};
		let banner = CachedAccountBanner {
			account_revision: 1,
			observed_at: 100,
			current: true,
			banner: AccountApiBannerState::Unsupported,
			context: None,
		};
		assert!(state.insert(outcome(Arc::clone(&generation), Some(banner.clone()))));
		let mut same_copy = banner.clone();
		same_copy.observed_at += 1;
		assert!(!state.insert(outcome(Arc::clone(&generation), Some(same_copy.clone()))));
		same_copy.context = Some(decodex_codex::AccountApiRecoveryContext {
			provider_account_id: "workspace-id".into(),
			plan_type: Some("team".into()),
		});
		assert!(state.insert(outcome(Arc::clone(&generation), Some(same_copy.clone()))));
		same_copy.context.as_mut().unwrap().plan_type = Some("pro".into());
		assert!(state.insert(outcome(Arc::clone(&generation), Some(same_copy))));
		assert!(state.insert(outcome(Arc::clone(&generation), None)));
		assert!(!state.banners[&account].current);
		state.invalidate_account(&account);
		assert!(!state.insert(outcome(generation, Some(banner.clone()))));
		assert!(!state.banners.contains_key(&account));
		let successor = state.cache_generation(&account);
		assert!(state.insert(outcome(successor, Some(banner))));
		assert!(state.retain_current(&HashMap::from([(account.clone(), 2)])));
		assert!(!state.banners.contains_key(&account));
	}
}

fn retain_last_good_inventory(
	current: &CachedResetCardInventory,
	requested_revision: i64,
	next: Result<ResetCardInventoryObservation, ResetCardServiceError>,
) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
	if current.account_revision != requested_revision {
		return next;
	}
	match (&current.result, &next) {
		(Ok(ResetCardInventoryObservation::Available(current)), Err(error))
			if should_retain_last_good_inventory(*error) =>
			Ok(ResetCardInventoryObservation::Available(current.clone())),
		(
			Ok(ResetCardInventoryObservation::Available(current)),
			Ok(ResetCardInventoryObservation::Available(next)),
		) if current.account_revision == next.account_revision
			&& current.details_complete
			&& !next.details_complete =>
		{
			let mut retained = current.clone();
			// The provider exposes quota and Reset Card details through separate reads. A partial
			// detail read must not replace the last coherent public inventory. Keep that complete
			// inventory visible while still publishing the newer independent quota facts. Reset
			// Card use does not trust this cache: the effect owner fetches and fences a fresh
			// complete provider inventory before dispatch.
			retained.five_hour_quota = next.five_hour_quota;
			retained.seven_day_quota = next.seven_day_quota;
			Ok(ResetCardInventoryObservation::Available(retained))
		},
		(
			Ok(ResetCardInventoryObservation::Available(current)),
			Ok(ResetCardInventoryObservation::ObservationFailed(failure)),
		) if should_retain_last_good_inventory(failure.error) =>
			Ok(ResetCardInventoryObservation::Available(current.clone())),
		_ => next,
	}
}

const fn should_retain_last_good_inventory(error: ResetCardServiceError) -> bool {
	matches!(
		error,
		ResetCardServiceError::ProviderUnavailable
			| ResetCardServiceError::VaultUnavailable
			| ResetCardServiceError::InventoryIncomplete
			| ResetCardServiceError::ProductStateUnavailable
	)
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
			(AccountQuotaDisposition::NotApplicable, AccountQuotaDisposition::NotApplicable) =>
				true,
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
	api: Option<Arc<AccountApiRuntime>>,
	profiles: Option<AccountProfileRuntime>,
	reset_cards: Option<ApiResetCardRuntime>,
	state: Arc<RwLock<AccountObservationState>>,
	refresh_requested: Arc<Notify>,
	observation_generation: watch::Sender<u64>,
}

impl AccountObservationService {
	#[cfg(test)]
	pub(crate) async fn cache_recovery_fixture(
		&self,
		account: AccountId,
		revision: i64,
		usage: decodex_codex::AccountApiUsage,
		provider: &str,
		user: &str,
	) {
		self.state.write().await.banners.insert(
			account,
			CachedAccountBanner {
				account_revision: revision,
				observed_at: current_unix_micros().expect("fixture clock"),
				current: true,
				banner: usage.banner_for(provider, user),
				context: usage.recovery_context_for(provider, user),
			},
		);
	}

	pub(crate) fn new(
		accounts: Arc<AccountService>,
		api: Option<Arc<AccountApiRuntime>>,
		profiles: Option<AccountProfileRuntime>,
		reset_cards: Option<ApiResetCardRuntime>,
	) -> Self {
		let (observation_generation, _) = watch::channel(0);
		Self {
			accounts,
			api,
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

	/// Read exact-revision recovery copy without making a provider request.
	pub(crate) async fn recovery(
		&self,
		account_id: &decodex_protocol::EntityId,
		revision: decodex_protocol::EntityRevision,
	) -> decodex_protocol::AccountRecoveryResult {
		let unavailable = || recovery::unavailable(account_id.clone(), revision);
		let Ok(id) = AccountId::new(account_id.as_str()) else {
			return unavailable();
		};
		let Ok(before) = self.accounts.inspect(&id).await else {
			return unavailable();
		};
		if before.account.tombstoned
			|| u64::try_from(before.account.revision).ok() != Some(revision.0)
		{
			return unavailable();
		}
		let cached = self.state.read().await.banners.get(&id).cloned();
		let Ok(after) = self.accounts.inspect(&id).await else {
			return unavailable();
		};
		if after.account.tombstoned || after.account.revision != before.account.revision {
			return unavailable();
		}
		let Some(now) = current_unix_micros() else {
			return unavailable();
		};
		recovery::project(account_id.clone(), revision, cached, now)
	}

	/// Read one last daemon-owned Reset Card value without contacting the provider.
	pub(crate) async fn reset_card_inventory(
		&self,
		account_id: &AccountId,
	) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
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
		let reset_card_wakeup =
			self.reset_cards.as_ref().map(ApiResetCardRuntime::observation_wakeup);
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
		// Observation scheduling only needs the account registry.  Reading routing here takes the
		// database-wide routing lock and conflicts with the quota fact writer, even though routing
		// cannot change which provider snapshot belongs to an account.  Keep Route reads on their
		// own capability path so a transient routing serialization failure cannot stop observation.
		let Ok(accounts) = self.accounts.list().await else {
			return;
		};
		let current = accounts
			.into_iter()
			.filter(|inspection| account_observation_is_schedulable(&inspection.account))
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
		let accounts = Arc::clone(&self.accounts);
		let api = self.api.clone();
		let profiles = self.profiles.clone();
		let task_account_id = account_id.clone();
		let task = observations.spawn(async move {
			let api_observation = match api {
				Some(api) => Some(api.observe_and_activate(&task_account_id).await),
				None => None,
			};
			let banner = api_observation
				.as_ref()
				.and_then(|observation| observation.inventory.as_ref().ok())
				.and_then(|inventory| {
					if matches!(inventory.banner, decodex_codex::AccountApiBannerState::Unavailable)
					{
						return None;
					}
					Some(CachedAccountBanner {
						account_revision: inventory.account_revision,
						observed_at: current_unix_micros()?,
						current: true,
						banner: inventory.banner.clone(),
						context: inventory.recovery_context.clone(),
					})
				});
			let (reset_cards, profile) = match api_observation {
				Some(observation) => {
					let reset_cards = Some(
						direct_inventory_observation(
							accounts.as_ref(),
							&task_account_id,
							account_revision,
							&observation,
						)
						.await,
					);
					let profile = match profiles {
						Some(profiles) => Some(match (observation.provider, observation.profile) {
							(Some(provider), Ok(profile)) =>
								profiles
									.observe_provider_profile(
										&task_account_id,
										observation.account_revision,
										provider,
										profile,
									)
									.await,
							(_, Err(error)) =>
								profiles
									.status_after_failure(
										&task_account_id,
										account_revision,
										map_profile_api_error(error),
									)
									.await,
							(None, Ok(_)) =>
								profiles
									.status_after_failure(
										&task_account_id,
										account_revision,
										AccountProfileRuntimeError::AccountChanged,
									)
									.await,
						}),
						None => None,
					};
					(reset_cards, profile)
				},
				None => {
					// No provider adapter means no provider observation. Keep the last daemon-owned
					// values visible and expose a bounded refresh error instead of blocking the UI.
					let reset_cards = Some(Err(ResetCardServiceError::ProviderUnavailable));
					let profile = match profiles {
						Some(profiles) => Some(
							profiles
								.status_after_failure(
									&task_account_id,
									account_revision,
									AccountProfileRuntimeError::ProviderUnavailable,
								)
								.await,
						),
						None => None,
					};
					(reset_cards, profile)
				},
			};
			AccountObservationOutcome {
				banner,
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

fn account_observation_is_schedulable(account: &decodex_core::AccountRecord) -> bool {
	!account.tombstoned && account.credential.is_some() && account.unsettled_operation.is_none()
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
		AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationKind,
		AccountOperationPhase, AccountOperationStatus, AccountProvider, AccountQuotaDisposition,
		AccountQuotaWindowObservation, AccountRecord, AccountState, CredentialBinding,
		CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
		ResetCardDescriptor, ResetCardTimestamp,
	};
	use decodex_database::AccountProfileSnapshot;
	use tokio::{sync::watch, time};

	use crate::account_launch::ResetCardInventoryView;

	use super::{
		AccountObservationOutcome, AccountObservationState, AccountProfileRefreshStatus,
		ResetCardInventoryObservation, ResetCardServiceError, account_observation_is_schedulable,
		plan_observation_round, resolve_direct_quota, wait_for_generation,
	};

	#[test]
	fn optional_absence_requires_successful_five_slot_and_current_weekly_fact() {
		use decodex_codex::AccountApiQuotaWindow;
		use decodex_core::{AccountQuotaObservationError, AccountQuotaWindow};
		let five = AccountApiQuotaWindow { duration_minutes: 300, result: Ok(None) };
		let weekly = AccountApiQuotaWindow {
			duration_minutes: 10_080,
			result: Ok(Some(AccountQuotaWindow::new(10_080, 8, 2_000_000).unwrap())),
		};
		assert!(super::has_optional_five_absence(&[five, weekly], 1000));
		assert!(!super::has_optional_five_absence(
			&[five, AccountApiQuotaWindow { result: Ok(None), ..weekly }],
			1000
		));
		assert!(!super::has_optional_five_absence(&[five, weekly], 2_000_000));
		for error in [
			AccountQuotaObservationError::ProviderUnavailable,
			AccountQuotaObservationError::ProtocolUnavailable,
		] {
			assert!(!super::has_optional_five_absence(
				&[AccountApiQuotaWindow { result: Err(error), ..five }, weekly],
				1000
			));
			assert!(!super::has_optional_five_absence(
				&[five, AccountApiQuotaWindow { result: Err(error), ..weekly }],
				1000
			));
		}
	}

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
	fn recovery_required_account_is_suppressed_on_every_scheduler_wake() {
		let account_id = account(9);
		let operation_id = AccountOperationId::new("22000000-0000-4000-8000-000000000099").unwrap();
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-9").unwrap();
		let mut record = AccountRecord {
			account_id,
			label: "Morgan".to_owned(),
			enabled: true,
			revision: 1,
			observed_state: AccountState::AuthFailed,
			lifecycle_readiness: AccountLifecycleReadiness::OperationUnsettled,
			credential: Some(CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(1).unwrap(),
				fingerprint: CredentialFingerprint::new("0".repeat(64)).unwrap(),
				provider,
				writer_operation_id: operation_id.clone(),
			}),
			unsettled_operation: Some(AccountOperationStatus {
				operation_id,
				kind: AccountOperationKind::Refresh,
				phase: AccountOperationPhase::RecoveryRequired,
				recovery_code: Some("provider_refresh_rejected".to_owned()),
			}),
			usage_observation: None,
			five_hour_quota: AccountQuotaWindowObservation::unknown(300).unwrap(),
			seven_day_quota: AccountQuotaWindowObservation::unknown(10_080).unwrap(),
			tombstoned: false,
		};

		assert!(!account_observation_is_schedulable(&record));
		assert!(!account_observation_is_schedulable(&record));
		record.unsettled_operation = None;
		record.lifecycle_readiness = AccountLifecycleReadiness::Ready;
		assert!(account_observation_is_schedulable(&record));
	}

	#[test]
	fn revision_change_prunes_both_daemon_owned_account_values() {
		let account_id = account(1);
		let mut state = AccountObservationState::default();
		let cache_generation = state.cache_generation(&account_id);
		state.insert(AccountObservationOutcome {
			banner: None,
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
			banner: None,
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
			banner: None,
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
			banner: None,
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
			banner: None,
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation: Arc::clone(&cache_generation),
			reset_cards: Some(Ok(available_inventory(&account_id, 100))),
			profile: None,
		}));

		assert!(!state.insert(AccountObservationOutcome {
			banner: None,
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
			banner: None,
			account_id: account_id.clone(),
			requested_revision: 12,
			cache_generation: Arc::clone(&cache_generation),
			reset_cards: None,
			profile: Some(profile_status(&account_id, 100)),
		}));

		assert!(!state.insert(AccountObservationOutcome {
			banner: None,
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

	#[test]
	fn transient_inventory_failure_keeps_the_last_good_snapshot_visible() {
		let account_id = account(5);
		let mut state = AccountObservationState::default();
		let cache_generation = state.cache_generation(&account_id);

		assert!(state.insert(AccountObservationOutcome {
			banner: None,
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation: Arc::clone(&cache_generation),
			reset_cards: Some(Ok(available_inventory(&account_id, 100))),
			profile: None,
		}));

		assert!(!state.insert(AccountObservationOutcome {
			banner: None,
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation,
			reset_cards: Some(Err(ResetCardServiceError::ProviderUnavailable)),
			profile: None,
		}));

		let cached = state.reset_cards.get(&account_id).expect("last good inventory");
		let Ok(ResetCardInventoryObservation::Available(inventory)) = &cached.result else {
			panic!("expected the available snapshot to remain cached");
		};
		assert_eq!(inventory.five_hour_quota.observed_at_unix_micros, Some(100));
	}

	#[test]
	fn incomplete_reset_credit_details_keep_last_complete_inventory_visible() {
		let account_id = account(6);
		let mut state = AccountObservationState::default();
		let cache_generation = state.cache_generation(&account_id);
		assert!(state.insert(AccountObservationOutcome {
			banner: None,
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation: Arc::clone(&cache_generation),
			reset_cards: Some(Ok(available_inventory(&account_id, 100))),
			profile: None,
		}));

		let incomplete = ResetCardInventoryObservation::Available(ResetCardInventoryView {
			account_id: account_id.clone(),
			account_revision: 11,
			reported_available_count: Some(1),
			details_complete: false,
			cards: Vec::new(),
			five_hour_quota: quota(300, 200, 40, 2_000_000),
			seven_day_quota: quota(10_080, 200, 50, 3_000_000),
		});
		assert!(state.insert(AccountObservationOutcome {
			banner: None,
			account_id: account_id.clone(),
			requested_revision: 11,
			cache_generation,
			reset_cards: Some(Ok(incomplete)),
			profile: None,
		}));

		let cached = state.reset_cards.get(&account_id).expect("last complete inventory");
		let Ok(ResetCardInventoryObservation::Available(inventory)) = &cached.result else {
			panic!("expected available inventory");
		};
		assert!(inventory.details_complete);
		assert_eq!(inventory.reported_available_count, Some(1));
		assert_eq!(inventory.cards.len(), 1);
		assert_eq!(inventory.five_hour_quota.observed_at_unix_micros, Some(200));
	}

	#[test]
	fn missing_optional_quota_window_retains_last_good_fact_and_freshness() {
		let cached = quota(300, 100, 20, 2_000_000);
		let (observed_at, disposition) = resolve_direct_quota(300, Ok(None), Some(cached), 456);

		assert_eq!(observed_at, Some(100));
		assert_eq!(disposition, cached.disposition);
	}

	#[test]
	fn missing_optional_quota_window_without_cache_is_unknown_not_an_error() {
		let (observed_at, disposition) = resolve_direct_quota(300, Ok(None), None, 456);

		assert_eq!(observed_at, None);
		assert_eq!(disposition, AccountQuotaDisposition::Unknown);
	}

	#[test]
	fn failed_optional_observation_does_not_create_or_refresh_absence() {
		let error = decodex_core::AccountQuotaObservationError::ProviderUnavailable;
		assert_eq!(
			resolve_direct_quota(300, Err(error), None, 456),
			(Some(456), AccountQuotaDisposition::Error(error))
		);
		let prior = AccountQuotaWindowObservation {
			duration_minutes: 300,
			observed_at_unix_micros: Some(100),
			disposition: AccountQuotaDisposition::NotApplicable,
		};
		assert_eq!(
			resolve_direct_quota(300, Err(error), Some(prior), 456),
			(Some(100), AccountQuotaDisposition::NotApplicable)
		);
		assert_eq!(
			resolve_direct_quota(300, Ok(None), Some(prior), 456),
			(Some(100), AccountQuotaDisposition::NotApplicable)
		);
	}

	#[test]
	fn missing_optional_quota_window_does_not_retain_an_old_error() {
		let cached = AccountQuotaWindowObservation {
			duration_minutes: 300,
			observed_at_unix_micros: Some(100),
			disposition: AccountQuotaDisposition::Error(
				decodex_core::AccountQuotaObservationError::UnsupportedWindow,
			),
		};
		let (observed_at, disposition) = resolve_direct_quota(300, Ok(None), Some(cached), 456);

		assert_eq!(observed_at, None);
		assert_eq!(disposition, AccountQuotaDisposition::Unknown);
	}

	fn available_inventory(
		account_id: &AccountId,
		observed_at_unix_micros: i64,
	) -> ResetCardInventoryObservation {
		let descriptor = ResetCardDescriptor::new(
			ResetCardTimestamp::from_unix_seconds(1_800_000_000).expect("valid grant"),
			ResetCardTimestamp::from_unix_seconds(1_800_003_600).expect("valid expiry"),
		)
		.expect("valid descriptor");
		ResetCardInventoryObservation::Available(ResetCardInventoryView {
			account_id: account_id.clone(),
			account_revision: 11,
			reported_available_count: Some(1),
			details_complete: true,
			cards: vec![descriptor],
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
