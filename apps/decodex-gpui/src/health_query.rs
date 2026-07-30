//! Presentation-neutral ownership of the bounded GPUI Health query.

use std::{
	collections::VecDeque,
	sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::Notify;

use decodex_protocol::{
	CURRENT_VERSION, DoctorReport, QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ServerId,
};

const MAX_CANCELLED_REQUESTS: usize = 8;

/// Bounded state rendered by the Health destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HealthSnapshot {
	pub(crate) load: HealthLoadState,
	pub(crate) report: Option<DoctorReport>,
	pub(crate) can_refresh: bool,
}

/// Closed query states. A retained report is never replaced by a failed observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HealthLoadState {
	NeverRequested,
	Loading,
	Ready,
	Offline,
	Stale,
	Refused,
}

/// Result disposition used to preserve other retained-session query owners.
pub(crate) enum HealthRouteOutcome {
	Fresh,
	Refused,
	Stale,
	Unmatched,
}

/// Cloneable Health controller. It owns no task, transport, or product authority.
#[derive(Clone)]
pub(crate) struct HealthQuery {
	inner: Arc<HealthQueryInner>,
}

struct HealthQueryInner {
	state: Mutex<QueryState>,
	notify: Notify,
}

impl HealthQuery {
	pub(crate) fn production() -> Self {
		Self {
			inner: Arc::new(HealthQueryInner {
				state: Mutex::new(QueryState::new()),
				notify: Notify::new(),
			}),
		}
	}

	/// Activate Health and request once for the current retained-session generation.
	pub(crate) fn activate(&self) {
		let mut state = self.lock();

		if state.active {
			return;
		}

		state.active = true;
		state.ever_activated = true;
		let queued = state.queue_first_activation();
		if state.session.is_none() {
			state.load = HealthLoadState::Offline;
		}
		drop(state);

		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn deactivate(&self) {
		self.lock().active = false;
	}

	/// Queue one explicit refresh only when no Health request is already pending.
	pub(crate) fn refresh(&self) -> bool {
		let mut state = self.lock();

		if !state.active {
			return false;
		}

		let queued = state.queue_request();
		drop(state);

		if queued {
			self.inner.notify.notify_one();
		}

		queued
	}

	pub(crate) fn snapshot(&self) -> HealthSnapshot {
		self.lock().snapshot()
	}

	/// Bind future Health dispatch to one exact retained-session generation and server.
	pub(crate) fn bind_session(&self, generation: u64, server_id: ServerId) {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id };

		if state.session.as_ref() == Some(&binding) {
			return;
		}

		state.cancel_in_flight();
		state.pending = None;
		state.session = Some(binding);
		state.requested_generation = None;

		if state.active && state.ever_activated {
			state.queue_first_activation();
		} else if state.ever_activated {
			state.load = HealthLoadState::Stale;
		} else {
			state.load = HealthLoadState::NeverRequested;
		}
		drop(state);
		self.inner.notify.notify_one();
	}

	/// Invalidate request ownership when its exact retained session ends.
	pub(crate) fn session_ended(&self, generation: u64) {
		let mut state = self.lock();

		if !state.session.as_ref().is_some_and(|session| session.generation == generation) {
			return;
		}

		state.cancel_in_flight();
		state.pending = None;
		state.session = None;
		state.requested_generation = None;
		state.load = if state.ever_activated {
			HealthLoadState::Offline
		} else {
			HealthLoadState::NeverRequested
		};
		drop(state);
		self.inner.notify.notify_one();
	}

	/// Wait for and reserve one exact outbound request. No polling or request queue is retained.
	pub(crate) async fn next_dispatch(
		&self,
		session_generation: u64,
		server_id: &ServerId,
	) -> HealthDispatch {
		loop {
			let notified = self.inner.notify.notified();

			if let Some(dispatch) = self.try_take_dispatch(session_generation, server_id) {
				return dispatch;
			}

			notified.await;
		}
	}

	fn try_take_dispatch(
		&self,
		session_generation: u64,
		server_id: &ServerId,
	) -> Option<HealthDispatch> {
		let mut state = self.lock();
		let expected =
			SessionBinding { generation: session_generation, server_id: server_id.clone() };

		if state.session.as_ref() != Some(&expected) || state.in_flight.is_some() {
			return None;
		}

		let pending = state.pending.take()?;
		if pending.session != expected {
			return None;
		}

		let envelope = QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: pending.query_id.clone(),
			payload: QueryPayload::GetDoctorStatus,
		};
		let dispatch =
			HealthDispatch { envelope, session_generation, server_id: server_id.clone() };

		state.in_flight = Some(InFlightRequest::from_dispatch(&dispatch));

		Some(dispatch)
	}

	/// Route only an exact Health result and leave every unmatched result for another owner.
	pub(crate) fn route_result(
		&self,
		session_generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> HealthRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight.as_ref() else {
			return if state.is_cancelled_query(&result.query_id) {
				HealthRouteOutcome::Stale
			} else {
				HealthRouteOutcome::Unmatched
			};
		};

		if in_flight.query_id != result.query_id {
			return if state.is_cancelled_query(&result.query_id) {
				HealthRouteOutcome::Stale
			} else {
				HealthRouteOutcome::Unmatched
			};
		}

		let expected =
			SessionBinding { generation: session_generation, server_id: server_id.clone() };
		let identity_matches = state.session.as_ref() == Some(&expected)
			&& in_flight.session == expected
			&& result.version == CURRENT_VERSION
			&& result.server_id == *server_id;
		let report = match &result.payload {
			QueryResultPayload::DoctorStatus(report)
				if identity_matches
					&& report.version() == CURRENT_VERSION
					&& report.server_id() == server_id
					&& report.has_current_component_set() =>
				Some(report.clone()),
			_ => None,
		};

		state.in_flight = None;
		if let Some(report) = report {
			state.report = Some(report);
			state.load = HealthLoadState::Ready;

			HealthRouteOutcome::Fresh
		} else {
			state.load = HealthLoadState::Refused;

			HealthRouteOutcome::Refused
		}
	}

	fn lock(&self) -> MutexGuard<'_, QueryState> {
		self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionBinding {
	generation: u64,
	server_id: ServerId,
}

struct QueryState {
	session: Option<SessionBinding>,
	active: bool,
	ever_activated: bool,
	requested_generation: Option<u64>,
	next_request_sequence: u64,
	pending: Option<PendingRequest>,
	in_flight: Option<InFlightRequest>,
	cancelled: VecDeque<CancelledRequest>,
	load: HealthLoadState,
	report: Option<DoctorReport>,
}

impl QueryState {
	fn new() -> Self {
		Self {
			session: None,
			active: false,
			ever_activated: false,
			requested_generation: None,
			next_request_sequence: 0,
			pending: None,
			in_flight: None,
			cancelled: VecDeque::new(),
			load: HealthLoadState::NeverRequested,
			report: None,
		}
	}

	fn queue_first_activation(&mut self) -> bool {
		let Some(session) = self.session.as_ref() else {
			return false;
		};
		if self.requested_generation == Some(session.generation) {
			return false;
		}

		self.requested_generation = Some(session.generation);

		self.queue_request()
	}

	fn queue_request(&mut self) -> bool {
		let Some(session) = self.session.clone() else {
			return false;
		};
		if self.pending.is_some() || self.in_flight.is_some() {
			return false;
		}

		let Some(request_sequence) = self.next_request_sequence.checked_add(1) else {
			self.load = HealthLoadState::Refused;

			return false;
		};
		self.next_request_sequence = request_sequence;
		let query_id =
			QueryId::new(format!("gpui-health/{}/{request_sequence}", session.generation))
				.expect("bounded numeric Health query identity");

		self.pending = Some(PendingRequest { session, query_id });
		self.load = HealthLoadState::Loading;

		true
	}

	fn cancel_in_flight(&mut self) {
		let Some(in_flight) = self.in_flight.take() else {
			return;
		};

		self.cancelled.push_back(CancelledRequest { query_id: in_flight.query_id });
		while self.cancelled.len() > MAX_CANCELLED_REQUESTS {
			self.cancelled.pop_front();
		}
	}

	fn is_cancelled_query(&self, query_id: &QueryId) -> bool {
		self.cancelled.iter().any(|request| &request.query_id == query_id)
	}

	fn snapshot(&self) -> HealthSnapshot {
		HealthSnapshot {
			load: self.load,
			report: self.report.clone(),
			can_refresh: self.active
				&& self.session.is_some()
				&& self.pending.is_none()
				&& self.in_flight.is_none(),
		}
	}
}

struct PendingRequest {
	session: SessionBinding,
	query_id: QueryId,
}

struct InFlightRequest {
	query_id: QueryId,
	session: SessionBinding,
}

impl InFlightRequest {
	fn from_dispatch(dispatch: &HealthDispatch) -> Self {
		Self {
			query_id: dispatch.envelope.query_id.clone(),
			session: SessionBinding {
				generation: dispatch.session_generation,
				server_id: dispatch.server_id.clone(),
			},
		}
	}
}

struct CancelledRequest {
	query_id: QueryId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HealthDispatch {
	envelope: QueryEnvelope,
	session_generation: u64,
	server_id: ServerId,
}

impl HealthDispatch {
	pub(crate) const fn envelope(&self) -> &QueryEnvelope {
		&self.envelope
	}
}

#[cfg(test)]
mod tests {
	use decodex_protocol::{
		CURRENT_VERSION, DoctorCheck, DoctorComponent, DoctorReport, DoctorStatus, QueryEnvelope,
		QueryId, QueryPayload, QueryResultEnvelope, QueryResultPayload, ServerId,
	};

	use super::{HealthDispatch, HealthLoadState, HealthQuery, HealthRouteOutcome, HealthSnapshot};

	fn server(value: &str) -> ServerId {
		ServerId::new(value).expect("fixture server ID must be bounded")
	}

	fn ready_report(server_id: &ServerId) -> DoctorReport {
		DoctorReport::new(
			server_id.clone(),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.map(|component| DoctorCheck::new(component, DoctorStatus::Ready))
				.collect(),
		)
		.expect("complete fixture report must be valid")
	}

	fn result(
		dispatch: &HealthDispatch,
		server_id: ServerId,
		report: DoctorReport,
	) -> QueryResultEnvelope {
		QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id,
			query_id: dispatch.envelope().query_id.clone(),
			payload: QueryResultPayload::DoctorStatus(report),
		}
	}

	#[test]
	fn first_activation_dispatches_once_then_fresh_result_and_refresh_coalesce() {
		let query = HealthQuery::production();
		let server_id = server("server-a");

		query.activate();
		assert_eq!(
			query.snapshot(),
			HealthSnapshot { load: HealthLoadState::Offline, report: None, can_refresh: false }
		);

		query.bind_session(7, server_id.clone());
		assert!(!query.refresh());
		let dispatch = query
			.try_take_dispatch(7, &server_id)
			.expect("first activation must reserve one dispatch");

		assert_eq!(
			dispatch.envelope(),
			&QueryEnvelope {
				version: CURRENT_VERSION,
				query_id: QueryId::new("gpui-health/7/1").unwrap(),
				payload: QueryPayload::GetDoctorStatus,
			}
		);
		assert!(query.try_take_dispatch(7, &server_id).is_none());
		assert!(!query.refresh());

		let report = ready_report(&server_id);
		assert!(matches!(
			query.route_result(
				7,
				&server_id,
				&result(&dispatch, server_id.clone(), report.clone()),
			),
			HealthRouteOutcome::Fresh
		));
		assert_eq!(
			query.snapshot(),
			HealthSnapshot {
				load: HealthLoadState::Ready,
				report: Some(report),
				can_refresh: true,
			}
		);

		assert!(query.refresh());
		assert!(!query.refresh());
		let refresh = query
			.try_take_dispatch(7, &server_id)
			.expect("coalesced refresh must reserve one dispatch");
		assert_eq!(&refresh.envelope().query_id, &QueryId::new("gpui-health/7/2").unwrap());
		assert!(query.try_take_dispatch(7, &server_id).is_none());
	}

	#[test]
	fn session_replacement_and_end_preserve_stale_and_unmatched_ownership() {
		let query = HealthQuery::production();
		let first_server = server("server-a");
		let next_server = server("server-b");

		query.bind_session(1, first_server.clone());
		query.activate();
		let stale_dispatch =
			query.try_take_dispatch(1, &first_server).expect("first session must own its dispatch");

		query.deactivate();
		query.bind_session(2, next_server.clone());
		assert_eq!(query.snapshot().load, HealthLoadState::Stale);
		assert!(matches!(
			query.route_result(
				1,
				&first_server,
				&result(&stale_dispatch, first_server.clone(), ready_report(&first_server),),
			),
			HealthRouteOutcome::Stale
		));

		let unmatched = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: next_server.clone(),
			query_id: QueryId::new("another-query-owner").unwrap(),
			payload: QueryResultPayload::DoctorStatus(ready_report(&next_server)),
		};
		assert!(matches!(
			query.route_result(2, &next_server, &unmatched),
			HealthRouteOutcome::Unmatched
		));

		query.session_ended(1);
		assert_eq!(query.snapshot().load, HealthLoadState::Stale);
		query.session_ended(2);
		assert_eq!(query.snapshot().load, HealthLoadState::Offline);
	}

	#[test]
	fn refused_refresh_preserves_the_retained_good_report() {
		let query = HealthQuery::production();
		let server_id = server("server-a");

		query.bind_session(3, server_id.clone());
		query.activate();
		let initial =
			query.try_take_dispatch(3, &server_id).expect("initial request must dispatch");
		let retained = ready_report(&server_id);
		assert!(matches!(
			query.route_result(
				3,
				&server_id,
				&result(&initial, server_id.clone(), retained.clone()),
			),
			HealthRouteOutcome::Fresh
		));

		assert!(query.refresh());
		let refresh =
			query.try_take_dispatch(3, &server_id).expect("refresh request must dispatch");
		let foreign_report = ready_report(&server("server-b"));
		assert!(matches!(
			query
				.route_result(3, &server_id, &result(&refresh, server_id.clone(), foreign_report),),
			HealthRouteOutcome::Refused
		));
		assert_eq!(
			query.snapshot(),
			HealthSnapshot {
				load: HealthLoadState::Refused,
				report: Some(retained),
				can_refresh: true,
			}
		);
	}
}
