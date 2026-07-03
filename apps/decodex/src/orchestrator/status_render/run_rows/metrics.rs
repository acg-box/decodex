use crate::orchestrator::{OperatorLaneLifecycleMetrics, status_render::activity};

pub(super) fn render_lane_lifecycle_metrics(metrics: &OperatorLaneLifecycleMetrics) -> String {
	format!(
		"attempts={}; sources=recorded:{},recovered:{},current_snapshot:{}; captured={}/{}; missing={}; protocol_events={}; child_events={}; wall={}; tool_calls={}; input_tokens={}; output_tokens={}",
		metrics.attempt_count,
		metrics.recorded_attempt_count,
		metrics.recovered_attempt_count,
		metrics.current_snapshot_attempt_count,
		metrics.captured_attempt_count,
		metrics.attempt_count,
		metrics.missing_attempt_count,
		metrics.protocol_event_count,
		metrics.child_event_count,
		activity::format_seconds_compact(metrics.wall_seconds),
		metrics.tool_call_count,
		metrics.input_tokens_cumulative,
		metrics.output_tokens_cumulative,
	)
}

pub(super) fn render_lane_lifecycle_evidence(metrics: &OperatorLaneLifecycleMetrics) -> String {
	if metrics.attempt_evidence.is_empty() && metrics.recovery_gaps.is_empty() {
		return String::from("none");
	}

	let mut lines = metrics
		.attempt_evidence
		.iter()
		.map(|attempt| {
			let evidence = if attempt.evidence.is_empty() {
				String::from("none")
			} else {
				attempt.evidence.join(",")
			};
			let gaps = if attempt.gaps.is_empty() {
				String::from("none")
			} else {
				attempt.gaps.join(",")
			};

			format!(
				"run={} attempt={} phase={} source={} evidence={} gaps={} protocol_events={} child_events={} updated_at={}",
				attempt.run_id,
				attempt.attempt_number,
				attempt.phase,
				attempt.source,
				evidence,
				gaps,
				attempt.protocol_event_count,
				attempt.child_event_count,
				attempt.updated_at
			)
		})
		.collect::<Vec<_>>();

	if !metrics.recovery_gaps.is_empty() {
		lines.push(format!("aggregate_gaps={}", metrics.recovery_gaps.join(",")));
	}

	lines.join(" | ")
}
