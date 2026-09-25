//! One retained daemon observation wait per session, shared across profile selections.

use super::{
	AccountProfileRouteOutcome, CURRENT_VERSION, QueryEnvelope, QueryId, QueryPayload,
	QueryResultEnvelope, QueryResultPayload, ServerId, SessionBinding, State,
};

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
				request_refresh: std::mem::take(&mut self.request_observation_refresh)
					.then_some(true),
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
			self.expire_recovery();
			return AccountProfileRouteOutcome::Refused;
		};
		if !valid {
			self.expire_recovery();
			return AccountProfileRouteOutcome::Refused;
		}
		if self.observation_generation != signal.generation {
			self.invalidate_actions();
		}
		self.observation_generation = signal.generation;
		if self.selected.is_some() {
			self.expire_recovery();
			// A heartbeat refreshes observation timestamps too. Coalesce with an active pair.
			self.refresh_due = !self.queue_query();
		}
		AccountProfileRouteOutcome::Fresh
	}
}

impl super::AccountProfileController {
	pub(crate) fn finish_observation(
		&self,
		generation: u64,
		server_id: &ServerId,
		query: QueryEnvelope,
		result: Result<decodex_protocol::AccountObservationSignal, decodex_protocol::ClientFailure>,
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
					state.expire_recovery();
				}
				drop(state);
				self.inner.notify.notify_one();
			},
		}
	}
}
