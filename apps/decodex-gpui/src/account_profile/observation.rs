//! One retained daemon observation wait per session, shared across profile selections.

use std::mem;

use crate::account_profile::{
	AccountProfileController, AccountProfileRouteOutcome, CURRENT_VERSION, QueryEnvelope, QueryId,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId, SessionBinding, State,
};
use decodex_protocol::{AccountObservationSignal, ClientFailure};

impl State {
	pub(super) fn start_observation(&mut self, binding: SessionBinding) -> Option<QueryEnvelope> {
		if self.observation.is_some() || self.selected.is_none() || self.selected_revision.is_none()
		{
			return None;
		}

		let sequence = self.next_sequence.checked_add(1)?;

		self.next_sequence = sequence;

		let query_id =
			QueryId::new(format!("gpui-account-observation/{}/{sequence}", binding.generation))
				.expect("bounded numeric observation query identity");

		self.observation = Some((query_id.clone(), binding));

		Some(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id,
			payload: QueryPayload::WaitForAccountObservation {
				after_generation: self.observation_generation,
				request_refresh: mem::take(&mut self.request_observation_refresh).then_some(true),
			},
		})
	}

	pub(super) fn accept_observation(
		&mut self,
		generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> AccountProfileRouteOutcome {
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		let valid = self.observation.as_ref().is_some_and(|(_, binding)| binding == &expected)
			&& self.session.as_ref() == Some(&expected)
			&& result.version == CURRENT_VERSION
			&& result.server_id == *server_id;

		self.observation = None;

		let QueryResultPayload::AccountObservation(signal) = &result.payload else {
			return AccountProfileRouteOutcome::Refused;
		};

		if !valid {
			return AccountProfileRouteOutcome::Refused;
		}

		self.observation_generation = signal.generation;

		if self.selected.is_some() {
			// A heartbeat refreshes observation timestamps too. Coalesce with an active profile
			// read.
			self.refresh_due = !self.queue_query();
		}

		AccountProfileRouteOutcome::Fresh
	}
}

impl AccountProfileController {
	pub(crate) fn finish_observation(
		&self,
		generation: u64,
		server_id: &ServerId,
		query: QueryEnvelope,
		result: Result<AccountObservationSignal, ClientFailure>,
	) {
		match result {
			Ok(signal) => {
				self.route_result(
					generation,
					server_id,
					&QueryResultEnvelope {
						version: CURRENT_VERSION,
						server_id: server_id.clone(),
						query_id: query.query_id,
						payload: QueryResultPayload::AccountObservation(signal),
					},
				);
			},
			Err(_) => {
				let mut state = self.lock();

				if state.observation.as_ref().is_some_and(|(id, binding)| {
					id == &query.query_id
						&& binding.generation == generation
						&& &binding.server_id == server_id
				}) {
					state.observation = None;
				}

				drop(state);

				self.inner.notify.notify_one();
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::account_profile::*;

	fn source() -> (AccountProfileController, ServerId, EntityId) {
		let controller = AccountProfileController::production();
		let server = ServerId::new("10000000-0000-4000-8000-000000000001").unwrap();
		let account = EntityId::new("20000000-0000-4000-8000-000000000001").unwrap();

		controller.bind_session(3, server.clone());
		controller.select_at_revision(account.clone(), EntityRevision(1));

		(controller, server, account)
	}

	fn complete_profile(controller: &AccountProfileController, server: &ServerId) {
		let profile = controller.try_take_dispatch(3, server).unwrap();

		assert!(matches!(profile.payload, QueryPayload::GetAccountProfile { .. }));

		let result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server.clone(),
			query_id: profile.query_id,
			payload: QueryResultPayload::AccountProfile(AccountProfileResult::Unavailable {
				error: decodex_protocol::AccountProfileErrorDto::ProviderUnavailable,
				email: AccountProfileEmailDto::Redacted,
				plan_type: None,
			}),
		};

		assert_eq!(controller.route_result(3, server, &result), AccountProfileRouteOutcome::Fresh);
	}

	#[test]
	fn observation_wait_is_single_across_refresh_close_and_reopen() {
		let (controller, server, account) = source();

		complete_profile(&controller, &server);

		let wait = controller.try_take_dispatch(3, &server).unwrap();

		assert!(matches!(
			wait.payload,
			QueryPayload::WaitForAccountObservation { after_generation: 0, .. }
		));
		assert!(controller.try_take_dispatch(3, &server).is_none());

		for _ in 0..4 {
			assert!(controller.refresh());

			complete_profile(&controller, &server);

			assert!(controller.try_take_dispatch(3, &server).is_none());
		}

		controller.close();

		assert!(controller.try_take_dispatch(3, &server).is_none());

		controller.select_at_revision(account.clone(), EntityRevision(1));

		complete_profile(&controller, &server);

		assert!(controller.try_take_dispatch(3, &server).is_none());

		let result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server.clone(),
			query_id: wait.query_id,
			payload: QueryResultPayload::AccountObservation(
				decodex_protocol::AccountObservationSignal::new(7),
			),
		};

		assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);

		complete_profile(&controller, &server);

		let wait = controller.try_take_dispatch(3, &server).unwrap();

		assert!(matches!(
			wait.payload,
			QueryPayload::WaitForAccountObservation { after_generation: 7, .. }
		));

		controller.close();

		let result = QueryResultEnvelope { query_id: wait.query_id, ..result };

		assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);
		assert!(controller.try_take_dispatch(3, &server).is_none());
	}

	#[test]
	fn observation_during_profile_read_coalesces_one_followup_read() {
		let (controller, server, _) = source();

		complete_profile(&controller, &server);

		let wait = controller.try_take_dispatch(3, &server).unwrap();

		controller.refresh();

		let result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server.clone(),
			query_id: wait.query_id,
			payload: QueryResultPayload::AccountObservation(
				decodex_protocol::AccountObservationSignal::new(1),
			),
		};

		assert_eq!(controller.route_result(3, &server, &result), AccountProfileRouteOutcome::Fresh);

		complete_profile(&controller, &server);
		complete_profile(&controller, &server);

		assert!(matches!(
			controller.try_take_dispatch(3, &server).unwrap().payload,
			QueryPayload::WaitForAccountObservation { .. }
		));
		assert!(controller.try_take_dispatch(3, &server).is_none());
	}
}
