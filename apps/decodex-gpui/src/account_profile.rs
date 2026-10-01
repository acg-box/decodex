//! Presentation-neutral ownership of one selected GPUI account-profile observation.

use std::{
	collections::VecDeque,
	sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::Notify;

use decodex_protocol::{
	AccountProfileEmailDto, AccountProfileResult, CURRENT_VERSION, EntityId, EntityRevision,
	QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId,
};

/// Bounded selected-profile state rendered by Accounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountProfileSnapshot {
	pub(crate) selected: Option<EntityId>,
	pub(crate) selected_revision: Option<EntityRevision>,
	pub(crate) load: AccountProfileLoadState,
	pub(crate) result: Option<AccountProfileResult>,
}

/// Cloneable account-profile controller with no transport or product authority.
#[derive(Clone)]
pub(crate) struct AccountProfileController {
	inner: Arc<AccountProfileInner>,
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
			state.selected = Some(account_id);
			state.selected_revision = revision;

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

	pub(crate) fn close(&self) {
		let mut state = self.lock();

		state.refresh_due = false;
		state.selected = None;
		state.selected_revision = None;

		state.pending.clear();

		state.in_flight = None;
		state.result = None;
		state.load = AccountProfileLoadState::Closed;
	}

	pub(crate) fn refresh(&self) -> bool {
		let mut state = self.lock();

		state.request_observation_refresh = true;

		state.pending.clear();

		state.in_flight = None;

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
		state.session = Some(binding);

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
		state.session = None;

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

			return AccountProfileRouteOutcome::Refused;
		}

		let valid = match (&in_flight.payload, &result.payload) {
			(
				QueryPayload::GetAccountProfile { account_id, .. },
				QueryResultPayload::AccountProfile(profile),
			) => profile_matches(profile, account_id, state.selected_revision),
			_ => false,
		};

		state.in_flight = None;

		if valid && let QueryResultPayload::AccountProfile(profile) = &result.payload {
			state.result = Some(profile.clone());
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

struct AccountProfileInner {
	state: Mutex<State>,
	notify: Notify,
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
	session: Option<SessionBinding>,
	selected: Option<EntityId>,
	selected_revision: Option<EntityRevision>,
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
			session: None,
			selected: None,
			selected_revision: None,
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
		AccountProfileSnapshot {
			selected: self.selected.clone(),
			selected_revision: self.selected_revision,
			load: self.load,
			result: self.result.clone(),
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

		let Some(sequence) = self.next_sequence.checked_add(1) else {
			self.load = AccountProfileLoadState::Refused;

			return false;
		};

		self.next_sequence = sequence;

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

/// Only definitive authentication failures require another login; a busy or
/// temporarily unavailable credential remains a recoverable warning.
pub(crate) fn requires_login(error: decodex_protocol::AccountProfileErrorDto) -> bool {
	use decodex_protocol::AccountProfileErrorDto::*;

	matches!(error, RefreshRejected | RefreshAmbiguous | AccessRejectedAfterRefresh | Unauthorized)
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
	async fn activity_reads_reject_old_revisions_and_then_wait_for_observation() {
		use decodex_protocol::{AccountProfileDto, EntityRevision, QueryPayload};

		let controller = AccountProfileController::production();
		let server = ServerId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let account = EntityId::new("20000000-0000-4000-8000-000000000001").unwrap();

		controller.bind_session(3, server.clone());
		controller.select_at_revision(account.clone(), EntityRevision(2));

		for revision in [1, 2] {
			if revision == 2 {
				controller.refresh();
			}

			let query = controller.next_dispatch(3, &server).await;

			assert!(matches!(query.payload, QueryPayload::GetAccountProfile { .. }));

			let result = AccountProfileResult::Current(Box::new(AccountProfileDto {
				account_id: account.clone(),
				account_revision: EntityRevision(revision),
				observed_at_unix_micros: 0,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
				display_name: None,
				username: None,
				lifetime_tokens: None,
				peak_daily_tokens: None,
				longest_task_seconds: None,
				current_streak_days: None,
				longest_streak_days: None,
				daily_usage: vec![],
			}));
			let outcome = controller.route_result(
				3,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::AccountProfile(result),
				},
			);

			assert_eq!(
				outcome,
				if revision == 1 {
					AccountProfileRouteOutcome::Refused
				} else {
					AccountProfileRouteOutcome::Fresh
				}
			);
		}

		let observation = controller.next_dispatch(3, &server).await;

		assert!(matches!(observation.payload, QueryPayload::WaitForAccountObservation { .. }));
	}

	#[tokio::test]
	async fn revision_change_and_refresh_discard_delayed_profile_results() {
		let controller = AccountProfileController::production();
		let server = ServerId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let account = EntityId::new("20000000-0000-4000-8000-000000000001").unwrap();

		controller.bind_session(3, server.clone());
		controller.select_at_revision(account.clone(), super::EntityRevision(1));

		let old = controller.next_dispatch(3, &server).await;

		controller.select_at_revision(account, super::EntityRevision(2));

		let newer = controller.next_dispatch(3, &server).await;
		let reply = |query: super::QueryEnvelope| QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server.clone(),
			query_id: query.query_id,
			payload: QueryResultPayload::AccountProfile(AccountProfileResult::Unavailable {
				error: AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
			}),
		};

		assert_eq!(
			controller.route_result(3, &server, &reply(old)),
			AccountProfileRouteOutcome::Unmatched
		);
		assert!(controller.refresh());
		assert_eq!(
			controller.route_result(3, &server, &reply(newer)),
			AccountProfileRouteOutcome::Unmatched
		);
		assert!(controller.snapshot().result.is_none());
		assert_eq!(controller.snapshot().load, AccountProfileLoadState::Loading);

		let current = controller.next_dispatch(3, &server).await;

		assert_eq!(
			controller.route_result(3, &server, &reply(current)),
			AccountProfileRouteOutcome::Fresh
		);
		assert_eq!(controller.snapshot().selected_revision, Some(super::EntityRevision(2)));
		assert_eq!(controller.snapshot().load, AccountProfileLoadState::Ready);
	}

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
#[path = "account_profile/observation.rs"] mod observation;
