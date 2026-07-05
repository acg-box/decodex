use std::time::Instant;

use time::OffsetDateTime;

use crate::{
	agent::app_server::{
		ChildActivityAccumulator,
		activity::{self, child::event},
	},
	state::ChildAgentActivitySummary,
};

impl ChildActivityAccumulator {
	pub(in crate::agent::app_server) fn record(
		&mut self,
		event_type: &str,
		payload: &str,
	) -> ChildAgentActivitySummary {
		let now = Instant::now();
		let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

		self.add_elapsed_time(now);

		let event = event::classify_child_activity_event(
			event_type,
			payload,
			self.active_tool_name.as_deref(),
		);

		self.summary.event_count += 1;
		self.summary.wall_seconds =
			activity::duration_seconds_i64(now.saturating_duration_since(self.started_at));

		self.record_event_bucket(&event);

		if let Some(tool_name) = event.tool_name.as_ref().filter(|_tool_name| event.tool_call) {
			self.active_tool_name = Some(tool_name.clone());
		}

		if event_type == "item/tool/call/response" {
			self.active_tool_name = None;
		}
		if event.completed {
			self.set_current(None, None, None);
		} else if let Some(next_bucket) = event.transition_bucket {
			self.set_current(Some(next_bucket), event.transition_detail, Some(now_unix_epoch));
		}

		self.summary.current_elapsed_seconds =
			self.summary.current_started_unix_epoch.and_then(|started_at| {
				now_unix_epoch.checked_sub(started_at).filter(|elapsed| *elapsed >= 0)
			});
		self.last_observed_at = now;

		self.summary.clone()
	}
}
