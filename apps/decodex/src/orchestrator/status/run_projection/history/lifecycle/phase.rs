use crate::orchestrator::{
	HashMap, OperatorLaneLifecyclePhaseMetrics, OperatorLifecycleMetricPhase, OperatorRunStatus,
	RUN_OPERATION_REVIEW_WRITEBACK, status_run_projection::history::lifecycle::totals,
};

pub(crate) fn operator_lane_lifecycle_phase_metrics(
	attempts: &[OperatorRunStatus],
) -> Vec<OperatorLaneLifecyclePhaseMetrics> {
	let mut groups = HashMap::<String, (String, u8, Vec<&OperatorRunStatus>)>::new();

	for run in attempts {
		let phase = operator_run_lifecycle_metric_phase(run);
		let entry = groups
			.entry(phase.key.to_owned())
			.or_insert_with(|| (phase.label.to_owned(), phase.rank, Vec::new()));

		entry.2.push(run);
	}

	let mut phases = groups
		.into_iter()
		.map(|(phase, (label, rank, runs))| {
			let totals = totals::operator_lane_lifecycle_totals(runs);

			(
				rank,
				OperatorLaneLifecyclePhaseMetrics {
					phase,
					label,
					attempt_count: totals.attempt_count,
					run_count: totals.run_count,
					recorded_attempt_count: totals.recorded_attempt_count,
					recovered_attempt_count: totals.recovered_attempt_count,
					current_snapshot_attempt_count: totals.current_snapshot_attempt_count,
					captured_attempt_count: totals.captured_attempt_count,
					missing_attempt_count: totals.missing_attempt_count,
					protocol_event_count: totals.protocol_event_count,
					child_event_count: totals.child_event_count,
					wall_seconds: totals.wall_seconds,
					tool_call_count: totals.tool_call_count,
					input_tokens_current: totals.input_tokens_current,
					input_tokens_peak: totals.input_tokens_peak,
					input_tokens_cumulative: totals.input_tokens_cumulative,
					output_tokens_cumulative: totals.output_tokens_cumulative,
					largest_tool_output_bytes: totals.largest_tool_output_bytes,
					largest_tool_output_tool: totals.largest_tool_output_tool,
					large_output_warnings: totals.large_output_warnings,
					buckets: totals.buckets,
					attempt_evidence: totals.attempt_evidence,
					recovery_gaps: totals.recovery_gaps,
				},
			)
		})
		.collect::<Vec<_>>();

	phases.sort_by(|(left_rank, left), (right_rank, right)| {
		left_rank.cmp(right_rank).then_with(|| left.phase.cmp(&right.phase))
	});

	phases.into_iter().map(|(_rank, phase)| phase).collect()
}

pub(crate) fn operator_run_lifecycle_metric_phase(
	run: &OperatorRunStatus,
) -> OperatorLifecycleMetricPhase {
	if matches!(
		run.status.as_str(),
		"cleanup_complete" | "closeout" | "closeout_pending" | "landed"
	) {
		return operator_lifecycle_metric_phase("closeout", "Closeout", 30);
	}
	if matches!(
		run.status.as_str(),
		"manual_attention" | "manual_attention_pending" | "needs_attention" | "terminal_failure"
	) || run.phase == "needs_attention"
	{
		return operator_lifecycle_metric_phase("manual_attention", "Manual attention", 40);
	}

	if let Some(review) = run
		.loop_status
		.as_ref()
		.and_then(|status| status.review.as_ref())
		.filter(|review| review.checkpoint.is_some() || review.status != "pending")
	{
		return match review.phase.as_str() {
			"repair" => operator_lifecycle_metric_phase("review_repair", "Review repair", 20),
			_ => operator_lifecycle_metric_phase("review", "Review", 10),
		};
	}

	if run.status == "review_repair_pending" {
		return operator_lifecycle_metric_phase("review_repair", "Review repair", 20);
	}
	if run.status == "review_handoff_pending"
		|| run.current_operation == RUN_OPERATION_REVIEW_WRITEBACK
	{
		return operator_lifecycle_metric_phase("review", "Review", 10);
	}

	operator_lifecycle_metric_phase("development", "Development", 0)
}

pub(crate) fn operator_lifecycle_metric_phase(
	key: &'static str,
	label: &'static str,
	rank: u8,
) -> OperatorLifecycleMetricPhase {
	OperatorLifecycleMetricPhase { key, label, rank }
}
