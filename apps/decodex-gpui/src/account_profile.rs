//! Presentation-neutral ownership of one selected GPUI account-profile observation.

use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::Notify;

use decodex_protocol::{
	AccountProfileResult, CURRENT_VERSION, EntityId, QueryEnvelope, QueryId, QueryPayload,
	QueryResultEnvelope, QueryResultPayload, ServerId,
};

/// Bounded selected-profile state rendered by Accounts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountProfileSnapshot {
	pub(crate) selected: Option<EntityId>,
	pub(crate) load: AccountProfileLoadState,
	pub(crate) result: Option<AccountProfileResult>,
	pub(crate) can_refresh: bool,
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

	pub(crate) fn select(&self, account_id: EntityId) {
		let mut state = self.lock();
		if state.selected.as_ref() != Some(&account_id) {
			state.selected = Some(account_id);
			state.pending = None;
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
		state.selected = None;
		state.pending = None;
		state.in_flight = None;
		state.result = None;
		state.load = AccountProfileLoadState::Closed;
	}

	pub(crate) fn refresh(&self) -> bool {
		let mut state = self.lock();
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
		state.pending = None;
		state.in_flight = None;
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
		state.pending = None;
		state.in_flight = None;
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
		let query = state.pending.take()?;
		state.in_flight = Some(InFlightQuery {
			query_id: query.query_id.clone(),
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
			return AccountProfileRouteOutcome::Refused;
		}
		state.in_flight = None;
		match &result.payload {
			QueryResultPayload::AccountProfile(profile) => {
				state.result = Some(profile.clone());
				state.load = AccountProfileLoadState::Ready;
				AccountProfileRouteOutcome::Fresh
			},
			_ => {
				state.load = AccountProfileLoadState::Refused;
				AccountProfileRouteOutcome::Refused
			},
		}
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
	query_id: QueryId,
	account_id: EntityId,
	binding: SessionBinding,
}

struct State {
	session: Option<SessionBinding>,
	selected: Option<EntityId>,
	next_sequence: u64,
	pending: Option<QueryEnvelope>,
	in_flight: Option<InFlightQuery>,
	load: AccountProfileLoadState,
	result: Option<AccountProfileResult>,
}

impl State {
	const fn new() -> Self {
		Self {
			session: None,
			selected: None,
			next_sequence: 0,
			pending: None,
			in_flight: None,
			load: AccountProfileLoadState::Closed,
			result: None,
		}
	}

	fn snapshot(&self) -> AccountProfileSnapshot {
		AccountProfileSnapshot {
			selected: self.selected.clone(),
			load: self.load,
			result: self.result.clone(),
			can_refresh: self.session.is_some()
				&& self.selected.is_some()
				&& self.pending.is_none()
				&& self.in_flight.is_none(),
		}
	}

	fn queue_query(&mut self) -> bool {
		let (Some(binding), Some(account_id)) = (&self.session, &self.selected) else {
			if self.selected.is_some() {
				self.load = AccountProfileLoadState::Offline;
			}
			return false;
		};
		if self.pending.is_some() || self.in_flight.is_some() {
			return false;
		}
		let Some(sequence) = self.next_sequence.checked_add(1) else {
			self.load = AccountProfileLoadState::Refused;
			return false;
		};
		self.next_sequence = sequence;
		self.pending = Some(QueryEnvelope {
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
