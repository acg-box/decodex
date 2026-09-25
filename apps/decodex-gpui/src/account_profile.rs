//! Presentation-neutral ownership of one selected GPUI account-profile observation.

use std::{
	collections::VecDeque,
	sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::Notify;

use decodex_protocol::{
	AccountProfileEmailDto, AccountProfileResult, AccountRecoveryResult, AccountRecoveryState,
	CURRENT_VERSION, EntityId, EntityRevision, QueryEnvelope, QueryId, QueryPayload,
	QueryResultEnvelope, QueryResultPayload, ServerId,
};

/// Bounded selected-profile state rendered by Accounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountProfileSnapshot {
	pub(crate) selected: Option<EntityId>,
	pub(crate) selected_revision: Option<EntityRevision>,
	pub(crate) recovery: Option<AccountRecoveryResult>,
	pub(crate) load: AccountProfileLoadState,
	pub(crate) result: Option<AccountProfileResult>,
	pub(crate) can_refresh: bool,
	pub(crate) notification_generation: u64,
}

/// Finite account-profile query state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountProfileLoadState {
	Closed,
	Loading,
	Ready,
	Offline,
	Refused,
}

/// Result disposition used to preserve every other retained-session query owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountProfileRouteOutcome {
	Fresh,
	Unmatched,
	Refused,
}

/// Cloneable account-profile controller with no transport or product authority.
#[derive(Clone)]
pub(crate) struct AccountProfileController {
	inner: Arc<AccountProfileInner>,
}

struct AccountProfileInner {
	state: Mutex<State>,
	notify: Notify,
}

impl AccountProfileController {
	pub(crate) fn production() -> Self {
		Self {
			inner: Arc::new(AccountProfileInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
			}),
		}
	}

	pub(crate) fn snapshot(&self) -> AccountProfileSnapshot {
		self.lock().snapshot()
	}

	#[cfg(test)]
	pub(crate) fn select(&self, account_id: EntityId) {
		self.select_source(account_id, None);
	}

	pub(crate) fn select_at_revision(&self, account_id: EntityId, revision: EntityRevision) {
		self.select_source(account_id, Some(revision));
	}

	fn select_source(&self, account_id: EntityId, revision: Option<EntityRevision>) {
		let mut state = self.lock();
		if state.selected.as_ref() == Some(&account_id) && state.selected_revision == revision {
			return;
		}
		{
			state.invalidate_actions();
			state.selected = Some(account_id);
			state.selected_revision = revision;
			state.recovery = None;
			state.dismissed_banner = None;
			state.pending.clear();
			state.in_flight = None;
			state.result = None;
		}
		let queued = state.queue_query();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn dismiss_recovery(&self, expected: &AccountRecoveryResult) -> bool {
		let mut state = self.lock();
		if state.snapshot().recovery.as_ref() != Some(expected) {
			return false;
		}
		let AccountRecoveryState::Current(banner) = &expected.state else {
			return false;
		};
		if !banner.dismissible {
			return false;
		}
		state.invalidate_actions();
		state.dismissed_banner = Some(banner.clone());
		true
	}

	pub(crate) fn close(&self) {
		let mut state = self.lock();
		state.invalidate_actions();
		state.refresh_due = false;
		state.selected = None;
		state.selected_revision = None;
		state.recovery = None;
		state.dismissed_banner = None;
		state.pending.clear();
		state.in_flight = None;
		state.result = None;
		state.load = AccountProfileLoadState::Closed;
	}

	pub(crate) fn refresh(&self) -> bool {
		let mut state = self.lock();
		state.invalidate_actions();
		state.request_observation_refresh = true;
		state.pending.clear();
		state.in_flight = None;
		state.expire_recovery();
		let queued = state.queue_query();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		queued
	}

	pub(crate) fn bind_session(&self, generation: u64, server_id: ServerId) {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id };
		if state.session.as_ref() == Some(&binding) {
			return;
		}
		state.pending.clear();
		state.in_flight = None;
		state.observation = None;
		state.observation_generation = 0;
		state.refresh_due = false;
		state.invalidate_actions();
		state.session = Some(binding);
		state.expire_recovery();
		let queued = state.queue_query();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn session_ended(&self, generation: u64) {
		let mut state = self.lock();
		if !state.session.as_ref().is_some_and(|binding| binding.generation == generation) {
			return;
		}
		state.pending.clear();
		state.in_flight = None;
		state.observation = None;
		state.refresh_due = false;
		state.invalidate_actions();
		state.session = None;
		state.expire_recovery();
		if state.selected.is_some() {
			state.load = AccountProfileLoadState::Offline;
		}
	}

	pub(crate) async fn next_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> QueryEnvelope {
		loop {
			let notified = self.inner.notify.notified();
			if let Some(query) = self.try_take_dispatch(generation, server_id) {
				return query;
			}
			notified.await;
		}
	}

	fn try_take_dispatch(&self, generation: u64, server_id: &ServerId) -> Option<QueryEnvelope> {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id: server_id.clone() };
		if state.session.as_ref() != Some(&binding) || state.in_flight.is_some() {
			return None;
		}
		let Some(query) = state.pending.pop_front() else {
			return state.start_observation(binding);
		};
		state.in_flight = Some(InFlightQuery {
			query_id: query.query_id.clone(),
			payload: query.payload.clone(),
			account_id: state.selected.clone()?,
			binding,
		});
		Some(query)
	}

	pub(crate) fn route_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> AccountProfileRouteOutcome {
		let mut state = self.lock();
		if state.observation.as_ref().is_some_and(|(id, _)| id == &result.query_id) {
			let outcome = state.accept_observation(generation, server_id, result);
			drop(state);
			self.inner.notify.notify_one();
			return outcome;
		}
		let Some(in_flight) = state.in_flight.as_ref() else {
			return AccountProfileRouteOutcome::Unmatched;
		};
		if in_flight.query_id != result.query_id {
			return AccountProfileRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| state.selected.as_ref() != Some(&in_flight.account_id)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
		{
			state.in_flight = None;
			state.load = AccountProfileLoadState::Refused;
			state.pending.clear();
			state.expire_recovery();
			return AccountProfileRouteOutcome::Refused;
		}
		let valid = match (&in_flight.payload, &result.payload) {
			(
				QueryPayload::GetAccountRecovery { account_id, account_revision },
				QueryResultPayload::AccountRecovery(recovery),
			) => recovery.valid_for(account_id, *account_revision),
			(
				QueryPayload::GetAccountProfile { account_id, .. },
				QueryResultPayload::AccountProfile(profile),
			) => profile_matches(profile, account_id, state.selected_revision),
			_ => false,
		};
		state.in_flight = None;
		if valid {
			match &result.payload {
				QueryResultPayload::AccountRecovery(recovery) => {
					if !actions::same_recovery_source(state.recovery.as_ref(), Some(recovery)) {
						state.invalidate_actions();
					}
					let retained = match &recovery.state {
						AccountRecoveryState::Current(banner)
						| AccountRecoveryState::Stale(banner) => state.dismissed_banner.as_ref() == Some(banner),
						AccountRecoveryState::Unavailable => true,
						_ => false,
					};
					if !retained {
						state.dismissed_banner = None;
					}
					state.recovery = Some(recovery.clone());
				},
				QueryResultPayload::AccountProfile(profile) => state.result = Some(profile.clone()),
				_ => unreachable!("validated account query kind"),
			}
		} else {
			state.expire_recovery();
		}
		if state.refresh_due {
			state.expire_recovery();
		}
		if state.refresh_due && state.pending.is_empty() {
			state.refresh_due = false;
			state.queue_query();
		}
		state.load = if !valid {
			AccountProfileLoadState::Refused
		} else if state.pending.is_empty() {
			AccountProfileLoadState::Ready
		} else {
			AccountProfileLoadState::Loading
		};
		drop(state);
		self.inner.notify.notify_one();
		if valid { AccountProfileRouteOutcome::Fresh } else { AccountProfileRouteOutcome::Refused }
	}

	fn lock(&self) -> MutexGuard<'_, State> {
		self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionBinding {
	generation: u64,
	server_id: ServerId,
}

struct InFlightQuery {
	payload: QueryPayload,
	query_id: QueryId,
	account_id: EntityId,
	binding: SessionBinding,
}

struct State {
	notification_generation: u64,
	interaction_epoch: u64,
	session: Option<SessionBinding>,
	selected: Option<EntityId>,
	selected_revision: Option<EntityRevision>,
	recovery: Option<AccountRecoveryResult>,
	dismissed_banner: Option<Box<decodex_protocol::AccountRecoveryBanner>>,
	next_sequence: u64,
	pending: VecDeque<QueryEnvelope>,
	in_flight: Option<InFlightQuery>,
	observation: Option<(QueryId, SessionBinding)>,
	observation_generation: u64,
	refresh_due: bool,
	request_observation_refresh: bool,
	load: AccountProfileLoadState,
	result: Option<AccountProfileResult>,
}

impl State {
	const fn new() -> Self {
		Self {
			notification_generation: 0,
			interaction_epoch: 0,
			session: None,
			selected: None,
			selected_revision: None,
			recovery: None,
			dismissed_banner: None,
			next_sequence: 0,
			pending: VecDeque::new(),
			in_flight: None,
			observation: None,
			observation_generation: 0,
			refresh_due: false,
			request_observation_refresh: false,
			load: AccountProfileLoadState::Closed,
			result: None,
		}
	}

	fn snapshot(&self) -> AccountProfileSnapshot {
		let mut recovery = self.recovery.clone();
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.ok()
			.and_then(|duration| i64::try_from(duration.as_micros()).ok());
		if let Some(result) = &mut recovery {
			let fresh = now
				.zip(result.observed_at_unix_micros)
				.is_some_and(|(now, at)| at <= now && now - at <= 300_000_000);
			if !fresh && let AccountRecoveryState::Current(banner) = &result.state {
				result.state = AccountRecoveryState::Stale(banner.clone());
			}
		}
		if recovery.as_ref().is_some_and(|result| match &result.state {
			AccountRecoveryState::Current(banner) | AccountRecoveryState::Stale(banner) =>
				self.dismissed_banner.as_ref() == Some(banner),
			_ => false,
		}) {
			recovery = None;
		}
		AccountProfileSnapshot {
			selected: self.selected.clone(),
			selected_revision: self.selected_revision,
			recovery,
			load: self.load,
			result: self.result.clone(),
			notification_generation: self.notification_generation,
			can_refresh: self.session.is_some()
				&& self.selected.is_some()
				&& self.pending.is_empty()
				&& self.in_flight.is_none(),
		}
	}

	fn invalidate_actions(&mut self) {
		self.interaction_epoch = self.interaction_epoch.saturating_add(1);
	}

	fn expire_recovery(&mut self) {
		if let Some(recovery) = &mut self.recovery
			&& let AccountRecoveryState::Current(banner) = &recovery.state
		{
			recovery.state = AccountRecoveryState::Stale(banner.clone());
		}
	}

	fn queue_query(&mut self) -> bool {
		let (Some(binding), Some(account_id)) = (&self.session, &self.selected) else {
			if self.selected.is_some() {
				self.load = AccountProfileLoadState::Offline;
			}
			return false;
		};
		if !self.pending.is_empty() || self.in_flight.is_some() {
			return false;
		}
		let Some(sequence) = self.next_sequence.checked_add(2) else {
			self.load = AccountProfileLoadState::Refused;
			return false;
		};
		self.next_sequence = sequence;
		if let Some(account_revision) = self.selected_revision {
			self.pending.push_back(QueryEnvelope {
				version: CURRENT_VERSION,
				query_id: QueryId::new(format!(
					"gpui-account-recovery/{}/{sequence}",
					binding.generation
				))
				.expect("bounded numeric recovery query identity"),
				payload: QueryPayload::GetAccountRecovery {
					account_id: account_id.clone(),
					account_revision,
				},
			});
		}
		self.pending.push_back(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new(format!(
				"gpui-account-profile/{}/{sequence}",
				binding.generation
			))
			.expect("bounded numeric account-profile query identity"),
			payload: QueryPayload::GetAccountProfile {
				account_id: account_id.clone(),
				include_email: false,
			},
		});
		self.load = AccountProfileLoadState::Loading;
		true
	}
}

fn profile_matches(
	profile: &AccountProfileResult,
	account: &EntityId,
	revision: Option<EntityRevision>,
) -> bool {
	match profile {
		AccountProfileResult::Current(profile) | AccountProfileResult::Cached { profile, .. } =>
			&profile.account_id == account
				&& revision.is_none_or(|revision| profile.account_revision == revision)
				&& matches!(profile.email, AccountProfileEmailDto::Redacted),
		AccountProfileResult::Unavailable { email, .. } =>
			matches!(email, AccountProfileEmailDto::Redacted),
	}
}

#[cfg(test)]
mod tests {
	use decodex_protocol::{
		AccountProfileEmailDto, AccountProfileErrorDto, AccountProfileResult, CURRENT_VERSION,
		EntityId, QueryResultEnvelope, QueryResultPayload, ServerId,
	};

	use super::{AccountProfileController, AccountProfileLoadState, AccountProfileRouteOutcome};

	#[tokio::test]
	async fn selected_profile_is_one_exact_retained_session_query() {
		let controller = AccountProfileController::production();
		let server =
			ServerId::new("10000000-0000-4000-8000-000000000001").expect("server identity");
		let account =
			EntityId::new("20000000-0000-4000-8000-000000000001").expect("account identity");
		controller.select(account.clone());
		assert_eq!(controller.snapshot().load, AccountProfileLoadState::Offline);
		controller.bind_session(3, server.clone());
		let query = controller.next_dispatch(3, &server).await;
		assert!(matches!(
			query.payload,
			decodex_protocol::QueryPayload::GetAccountProfile { account_id, include_email: false }
				if account_id == account
		));
		assert_eq!(
			controller.route_result(
				3,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::AccountProfile(
						AccountProfileResult::Unavailable {
							error: AccountProfileErrorDto::ProviderUnavailable,
							email: AccountProfileEmailDto::Redacted,
							plan_type: None,
						}
					),
				},
			),
			AccountProfileRouteOutcome::Fresh
		);
		assert_eq!(controller.snapshot().load, AccountProfileLoadState::Ready);
	}

	#[tokio::test]
	async fn newer_profile_selection_supersedes_the_in_flight_query() {
		let controller = AccountProfileController::production();
		let server =
			ServerId::new("10000000-0000-4000-8000-000000000001").expect("server identity");
		let first = EntityId::new("20000000-0000-4000-8000-000000000001").expect("first account");
		let second = EntityId::new("20000000-0000-4000-8000-000000000002").expect("second account");
		controller.bind_session(3, server.clone());
		controller.select(first);
		let first_query = controller.next_dispatch(3, &server).await;

		controller.select(second.clone());
		let second_query = controller.next_dispatch(3, &server).await;
		assert_ne!(first_query.query_id, second_query.query_id);
		assert!(matches!(
			second_query.payload,
			decodex_protocol::QueryPayload::GetAccountProfile { account_id, .. }
				if account_id == second
		));

		let late_first = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server.clone(),
			query_id: first_query.query_id,
			payload: QueryResultPayload::AccountProfile(AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
			}),
		};
		assert_eq!(
			controller.route_result(3, &server, &late_first),
			AccountProfileRouteOutcome::Unmatched
		);
		assert_eq!(controller.snapshot().load, AccountProfileLoadState::Loading);

		let current_second = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server.clone(),
			query_id: second_query.query_id,
			payload: QueryResultPayload::AccountProfile(AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
			}),
		};
		assert_eq!(
			controller.route_result(3, &server, &current_second),
			AccountProfileRouteOutcome::Fresh
		);
		assert_eq!(controller.snapshot().selected, Some(second));
		assert_eq!(controller.snapshot().load, AccountProfileLoadState::Ready);
	}
}

#[cfg(test)]
#[path = "account_profile_recovery_tests.rs"]
mod recovery_tests;

#[path = "account_profile/observation.rs"] mod observation;

/// Substitute only the native time placeholder; keep provider copy as plain text.
pub(crate) fn recovery_copy(text: &str, reset_at: Option<i64>) -> String {
	let Some(reset) = reset_at.and_then(|at| time::OffsetDateTime::from_unix_timestamp(at).ok())
	else {
		return text.to_owned();
	};
	let (year, month, day) = reset.to_calendar_date();
	let label = format!(
		"{year:04}-{:02}-{day:02} {:02}:{:02} UTC",
		month as u8,
		reset.hour(),
		reset.minute()
	);
	text.replace("{time}", &label)
}

#[path = "account_profile/actions.rs"] mod actions;
pub(crate) use actions::RecoveryActionTicket;
