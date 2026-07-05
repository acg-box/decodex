use std::time::Instant;

use crate::{
	agent::app_server::{
		ChildActivityAccumulator,
		activity::{
			self, LARGE_CHILD_OUTPUT_BYTES,
			child::{
				bucket,
				model::{ChildActivityEvent, LargeOutputStats},
			},
		},
	},
	state::ChildAgentActivityBucket,
};

impl ChildActivityAccumulator {
	pub(in crate::agent::app_server::activity::child::accumulator) fn add_elapsed_time(
		&mut self,
		now: Instant,
	) {
		let Some(bucket_name) = self.current_bucket.clone() else {
			return;
		};
		let seconds =
			activity::duration_seconds_i64(now.saturating_duration_since(self.last_observed_at));
		let bucket = bucket::child_activity_bucket_mut(&mut self.summary, &bucket_name);

		bucket.wall_seconds = bucket.wall_seconds.saturating_add(seconds);
	}

	pub(in crate::agent::app_server::activity::child::accumulator) fn record_event_bucket(
		&mut self,
		event: &ChildActivityEvent,
	) {
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
			.collect::<Vec<(String, LargeOutputStats)>>();

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
}
