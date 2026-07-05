mod buckets;
mod current;
mod record;

use std::{collections::HashMap, time::Instant};

use crate::{
	agent::app_server::activity::child::model::LargeOutputStats, state::ChildAgentActivitySummary,
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
}
