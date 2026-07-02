use std::time::Duration;

use crate::{
	agent::app_server::{
		MODEL_EXECUTION_IDLE_TIMEOUT,
		activity::{
			RECENT_PROTOCOL_ACTIVITY_LIMIT,
			protocol::{event, status},
		},
	},
	state::{ChildAgentActivitySummary, ProtocolActivitySummary},
};

pub(in crate::agent::app_server) struct ProtocolActivityAccumulator {
	pub(in crate::agent::app_server) summary: ProtocolActivitySummary,
}
impl ProtocolActivityAccumulator {
	pub(in crate::agent::app_server) fn new() -> Self {
		Self { summary: ProtocolActivitySummary::default() }
	}

	pub(in crate::agent::app_server) fn record(
		&mut self,
		event_type: &str,
		payload: &str,
		child_activity: &ChildAgentActivitySummary,
	) -> ProtocolActivitySummary {
		self.summary.recent_events.push(event::protocol_activity_event(event_type, payload));

		if self.summary.recent_events.len() > RECENT_PROTOCOL_ACTIVITY_LIMIT {
			let remove_count =
				self.summary.recent_events.len().saturating_sub(RECENT_PROTOCOL_ACTIVITY_LIMIT);

			self.summary.recent_events.drain(0..remove_count);
		}

		if let Some(turn_status) = status::protocol_turn_status_from_payload(event_type, payload) {
			self.summary.turn_status = Some(turn_status);
		}
		if let Some(waiting_reason) =
			status::protocol_waiting_reason(event_type, payload, child_activity)
		{
			self.summary.waiting_reason = Some(waiting_reason);
		}
		if let Some(rate_limit_status) = status::protocol_rate_limit_status(event_type, payload) {
			self.summary.rate_limit_status = Some(rate_limit_status);
		}

		self.summary.clone()
	}
}

pub(crate) fn protocol_activity_idle_timeout(
	protocol_activity: Option<&ProtocolActivitySummary>,
	base_timeout: Duration,
) -> Duration {
	if protocol_activity.is_some_and(status::running_model_execution_protocol_activity) {
		return base_timeout.max(MODEL_EXECUTION_IDLE_TIMEOUT);
	}

	base_timeout
}
