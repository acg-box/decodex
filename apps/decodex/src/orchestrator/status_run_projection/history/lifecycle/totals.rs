use crate::orchestrator::{
	ChildAgentActivityBucket, HashMap, HashSet, OperatorLaneLifecycleMetrics, OperatorRunStatus,
	status_run_projection,
	status_run_projection::history::lifecycle::{evidence, phase},
};

pub(crate) fn operator_lane_lifecycle_metrics(
	attempts: &[OperatorRunStatus],
) -> OperatorLaneLifecycleMetrics {
	let mut metrics = operator_lane_lifecycle_totals(attempts.iter());

	metrics.phases = phase::operator_lane_lifecycle_phase_metrics(attempts);

	metrics
}

pub(crate) fn operator_lane_lifecycle_totals<'a>(
	runs: impl IntoIterator<Item = &'a OperatorRunStatus>,
) -> OperatorLaneLifecycleMetrics {
	let mut bucket_totals = HashMap::<String, ChildAgentActivityBucket>::new();
	let mut warning_set = HashSet::<String>::new();
	let mut run_ids = HashSet::<String>::new();
	let mut metrics = OperatorLaneLifecycleMetrics::default();

	for run in runs {
		metrics.attempt_count += 1;

		run_ids.insert(run.run_id.clone());

		match run.lifecycle_source.as_str() {
			"recorded" => metrics.recorded_attempt_count += 1,
			"recovered" => metrics.recovered_attempt_count += 1,
			"current_snapshot" => metrics.current_snapshot_attempt_count += 1,
			_ => {},
		}

		metrics.recovery_gaps.extend(run.lifecycle_gaps.iter().cloned());
		metrics.attempt_evidence.push(evidence::operator_lane_lifecycle_attempt_evidence(run));

		metrics.protocol_event_count =
			metrics.protocol_event_count.saturating_add(run.event_count.max(0));

		let Some(summary) = run.child_agent_activity.as_ref() else {
			continue;
		};

		metrics.captured_attempt_count += 1;
		metrics.child_event_count =
			metrics.child_event_count.saturating_add(summary.event_count.max(0));
		metrics.wall_seconds = metrics.wall_seconds.saturating_add(summary.wall_seconds.max(0));
		metrics.tool_call_count =
			metrics.tool_call_count.saturating_add(summary.tool_call_count.max(0));
		metrics.input_tokens_current = status_run_projection::max_optional_i64(
			metrics.input_tokens_current,
			summary.input_tokens_current,
		);
		metrics.input_tokens_peak = status_run_projection::max_optional_i64(
			metrics.input_tokens_peak,
			summary.input_tokens_max,
		);
		metrics.input_tokens_cumulative =
			metrics.input_tokens_cumulative.saturating_add(summary.input_tokens_cumulative.max(0));
		metrics.output_tokens_cumulative = metrics
			.output_tokens_cumulative
			.saturating_add(summary.output_tokens_cumulative.max(0));

		if summary.largest_tool_output_bytes.is_some_and(|bytes| {
			metrics.largest_tool_output_bytes.is_none_or(|current| bytes > current)
		}) {
			metrics.largest_tool_output_bytes = summary.largest_tool_output_bytes;
			metrics.largest_tool_output_tool = summary.largest_tool_output_tool.clone();
		}

		for warning in &summary.large_output_warnings {
			if !warning.trim().is_empty() {
				warning_set.insert(warning.clone());
			}
		}
		for bucket in &summary.buckets {
			let total = bucket_totals.entry(bucket.name.clone()).or_insert_with(|| {
				ChildAgentActivityBucket {
					name: bucket.name.clone(),
					..ChildAgentActivityBucket::default()
				}
			});

			total.wall_seconds = total.wall_seconds.saturating_add(bucket.wall_seconds.max(0));
			total.event_count = total.event_count.saturating_add(bucket.event_count.max(0));
			total.tool_call_count =
				total.tool_call_count.saturating_add(bucket.tool_call_count.max(0));
			total.input_tokens = total.input_tokens.saturating_add(bucket.input_tokens.max(0));
			total.output_tokens = total.output_tokens.saturating_add(bucket.output_tokens.max(0));
			total.output_bytes = total.output_bytes.saturating_add(bucket.output_bytes.max(0));
		}
	}

	metrics.missing_attempt_count =
		metrics.attempt_count.saturating_sub(metrics.captured_attempt_count);
	metrics.run_count = run_ids.len();
	metrics.large_output_warnings = warning_set.into_iter().collect();

	metrics.recovery_gaps.sort();
	metrics.recovery_gaps.dedup();
	metrics.attempt_evidence.sort_by(|left, right| {
		left.attempt_number.cmp(&right.attempt_number).then_with(|| left.run_id.cmp(&right.run_id))
	});
	metrics.large_output_warnings.sort();

	metrics.buckets = bucket_totals.into_values().collect();

	metrics.buckets.sort_by(|left, right| {
		right
			.wall_seconds
			.cmp(&left.wall_seconds)
			.then_with(|| right.event_count.cmp(&left.event_count))
			.then_with(|| left.name.cmp(&right.name))
	});

	metrics
}
