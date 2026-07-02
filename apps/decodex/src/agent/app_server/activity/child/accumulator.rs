use std::{collections::HashMap, time::Instant};

use time::OffsetDateTime;

use crate::{
	agent::app_server::activity::{
		self, LARGE_CHILD_OUTPUT_BYTES,
		child::{
			bucket, event,
			model::{ChildActivityEvent, LargeOutputStats},
		},
	},
	state::{ChildAgentActivityBucket, ChildAgentActivitySummary},
};

pub(in crate::agent::app_server) struct ChildActivityAccumulator {
	started_at: Instant,
	last_observed_at: Instant,
	current_bucket: Option<String>,
	current_detail: Option<String>,
	active_tool_name: Option<String>,
	large_output_stats: HashMap<String, LargeOutputStats>,
	summary: ChildAgentActivitySummary,
}
impl ChildActivityAccumulator {
	pub(in crate::agent::app_server) fn new() -> Self {
		let now = Instant::now();

		Self {
			started_at: now,
			last_observed_at: now,
			current_bucket: None,
			current_detail: None,
			active_tool_name: None,
			large_output_stats: HashMap::new(),
			summary: ChildAgentActivitySummary::default(),
		}
	}

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

	fn add_elapsed_time(&mut self, now: Instant) {
		let Some(bucket_name) = self.current_bucket.clone() else {
			return;
		};
		let seconds =
			activity::duration_seconds_i64(now.saturating_duration_since(self.last_observed_at));
		let bucket = bucket::child_activity_bucket_mut(&mut self.summary, &bucket_name);

		bucket.wall_seconds = bucket.wall_seconds.saturating_add(seconds);
	}

	fn record_event_bucket(&mut self, event: &ChildActivityEvent) {
		{
			let bucket: &mut ChildAgentActivityBucket =
				bucket::child_activity_bucket_mut(&mut self.summary, &event.event_bucket);

			bucket.event_count += 1;

			if event.tool_call {
				bucket.tool_call_count += 1;
			}

			if let Some(input_tokens) = event.input_tokens {
				bucket.input_tokens = bucket.input_tokens.saturating_add(input_tokens);
			}
			if let Some(output_tokens) = event.output_tokens {
				bucket.output_tokens = bucket.output_tokens.saturating_add(output_tokens);
			}
			if let Some(output_bytes) = event.tool_output_bytes {
				bucket.output_bytes = bucket.output_bytes.saturating_add(output_bytes);
			}
		}

		if event.tool_call {
			self.summary.tool_call_count += 1;
		}

		if let Some(input_tokens) = event.input_tokens {
			self.summary.input_tokens_current = Some(input_tokens);
			self.summary.input_tokens_max = Some(
				self.summary
					.input_tokens_max
					.map_or(input_tokens, |max_tokens| max_tokens.max(input_tokens)),
			);
			self.summary.input_tokens_cumulative =
				self.summary.input_tokens_cumulative.saturating_add(input_tokens);
		}
		if let Some(output_tokens) = event.output_tokens {
			self.summary.output_tokens_cumulative =
				self.summary.output_tokens_cumulative.saturating_add(output_tokens);
		}
		if let Some(output_bytes) = event.tool_output_bytes {
			self.record_tool_output(event, output_bytes);
		}
	}

	fn record_tool_output(&mut self, event: &ChildActivityEvent, output_bytes: i64) {
		let tool_name =
			event.tool_name.as_deref().or(event.event_detail.as_deref()).unwrap_or("tool");

		if self.summary.largest_tool_output_bytes.is_none_or(|largest| output_bytes > largest) {
			self.summary.largest_tool_output_bytes = Some(output_bytes);
			self.summary.largest_tool_output_tool = Some(tool_name.to_owned());
		}
		if output_bytes < LARGE_CHILD_OUTPUT_BYTES {
			return;
		}

		let stats = self.large_output_stats.entry(tool_name.to_owned()).or_default();

		stats.count += 1;
		stats.max_bytes = stats.max_bytes.max(output_bytes);

		self.refresh_large_output_warnings();
	}

	fn refresh_large_output_warnings(&mut self) {
		let mut entries = self
			.large_output_stats
			.iter()
			.map(|(tool_name, stats)| (tool_name.clone(), *stats))
			.collect::<Vec<_>>();

		entries.sort_by(|left, right| {
			right.1.max_bytes.cmp(&left.1.max_bytes).then_with(|| left.0.cmp(&right.0))
		});

		self.summary.large_output_warnings = entries
			.into_iter()
			.take(4)
			.map(|(tool_name, stats)| {
				if stats.count > 1 {
					format!(
						"{tool_name} repeated {} large outputs; largest {} bytes",
						stats.count, stats.max_bytes
					)
				} else {
					format!("{tool_name} produced a large output: {} bytes", stats.max_bytes)
				}
			})
			.collect();
	}

	fn set_current(
		&mut self,
		bucket: Option<String>,
		detail: Option<String>,
		started_unix_epoch: Option<i64>,
	) {
		if self.current_bucket == bucket && self.current_detail == detail {
			return;
		}

		self.current_bucket = bucket.clone();
		self.current_detail = detail.clone();
		self.summary.current_bucket = bucket;
		self.summary.current_detail = detail;
		self.summary.current_started_unix_epoch = started_unix_epoch;
		self.summary.current_elapsed_seconds = None;
	}
}
