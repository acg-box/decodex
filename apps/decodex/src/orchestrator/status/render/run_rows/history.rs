use crate::orchestrator::{
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome,
	status_render::{
		activity,
		run_rows::{metrics, run},
	},
};

pub(in crate::orchestrator::status::render) fn append_rendered_history_lane(
	output: &mut String,
	lane: &OperatorHistoryLaneStatus,
) {
	output.push_str(&format!(
		"- issue: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempts: {}\n  ledger_status: {}\n  outcome: {}\n",
		lane.issue_key,
		lane.project_id,
		lane.issue_id,
		lane.issue_identifier.as_deref().unwrap_or("none"),
		lane.title.as_deref().unwrap_or("none"),
		lane.attempt_count,
		lane.ledger_outcome.ledger_status,
		lane.ledger_outcome.final_outcome
	));

	append_rendered_history_ledger_outcome(output, &lane.ledger_outcome);

	output.push_str(&format!(
		"  lifecycle_metrics: {}\n",
		metrics::render_lane_lifecycle_metrics(&lane.lifecycle_metrics)
	));

	if history_ledger_outcome_has_records(&lane.ledger_outcome) {
		output.push_str(&format!(
			"  local_attempts: {}\n  latest_run_id: {}\n",
			lane.attempt_count, lane.latest_run.run_id
		));
	} else {
		run::append_rendered_run(output, &lane.latest_run);
	}
	if lane.lifecycle_metrics.phases.is_empty() {
		return;
	}

	output.push_str("  lifecycle_bucket_breakdown:\n");

	for phase in &lane.lifecycle_metrics.phases {
		output.push_str(&format!(
			"    - lifecycle_bucket: {} lifecycle_bucket_key: {} attempts: {} sources: recorded={} recovered={} current_snapshot={} captured: {}/{} protocol_events: {} child_events: {} wall: {} tool_calls: {} input_tokens: {} output_tokens: {}\n",
			phase.label,
			phase.phase,
			phase.attempt_count,
			phase.recorded_attempt_count,
			phase.recovered_attempt_count,
			phase.current_snapshot_attempt_count,
			phase.captured_attempt_count,
			phase.attempt_count,
			phase.protocol_event_count,
			phase.child_event_count,
			activity::format_seconds_compact(phase.wall_seconds),
			phase.tool_call_count,
			phase.input_tokens_cumulative,
			phase.output_tokens_cumulative,
		));
	}
}

fn append_rendered_history_ledger_outcome(
	output: &mut String,
	outcome: &OperatorHistoryLedgerOutcome,
) {
	append_rendered_history_field(output, "event_type", outcome.final_event_type.as_deref());
	append_rendered_history_field(output, "event_at", outcome.final_event_at.as_deref());
	append_rendered_history_field(output, "summary", outcome.summary.as_deref());
	append_rendered_history_field(output, "pr_url", outcome.pr_url.as_deref());
	append_rendered_history_field(output, "commit_sha", outcome.commit_sha.as_deref());
	append_rendered_history_field(output, "branch", outcome.branch.as_deref());
	append_rendered_history_field(output, "closeout_status", outcome.closeout_status.as_deref());
	append_rendered_history_field(
		output,
		"needs_attention_reason",
		outcome.needs_attention_reason.as_deref(),
	);
	append_rendered_history_field(
		output,
		"lifecycle_started_at",
		outcome.lifecycle_started_at.as_deref(),
	);
	append_rendered_history_field(
		output,
		"lifecycle_finished_at",
		outcome.lifecycle_finished_at.as_deref(),
	);

	if let Some(elapsed) = outcome.lifecycle_elapsed_seconds {
		output.push_str(&format!("  lifecycle_elapsed_seconds: {elapsed}\n"));
	}

	output.push_str(&format!("  ledger_records: {}\n", outcome.record_count));
}

fn append_rendered_history_field(output: &mut String, label: &str, value: Option<&str>) {
	if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
		output.push_str(&format!("  {label}: {value}\n"));
	}
}

fn history_ledger_outcome_has_records(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	matches!(outcome.ledger_status.as_str(), "present" | "partial")
}
