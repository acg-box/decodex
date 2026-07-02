use serde_json::Value;

use crate::orchestrator::PrivateExecutionEvent;
use crate::orchestrator::agent_evidence::PrivateEvidenceRepoGateFailureSummary;

pub(super) fn repo_gate_failures_from_private_events(
	events: &[PrivateExecutionEvent],
) -> Vec<PrivateEvidenceRepoGateFailureSummary> {
	events.iter().filter_map(repo_gate_failure_from_private_event).collect()
}

fn repo_gate_failure_from_private_event(
	event: &PrivateExecutionEvent,
) -> Option<PrivateEvidenceRepoGateFailureSummary> {
	if event.event_type() != "phase_goal_transition" {
		return None;
	}

	let payload = event.payload();
	let transition_payload = payload.get("payload")?;
	let error_class = transition_payload.get("errorClass")?.as_str()?.to_owned();

	if !error_class.starts_with("repo_gate_") {
		return None;
	}

	let diagnostic = transition_payload.get("repoGateFailure");

	Some(PrivateEvidenceRepoGateFailureSummary {
		record_id: event.record_id(),
		phase: payload.get("phase")?.as_str()?.to_owned(),
		error_class,
		disposition: transition_payload.get("disposition")?.as_str()?.to_owned(),
		stage: diagnostic
			.and_then(|value| value.get("stage"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		failed_command: diagnostic
			.and_then(|value| value.get("failed_command"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		exit_status: diagnostic.and_then(|value| value.get("exit_status")).and_then(Value::as_i64),
		summary: diagnostic
			.and_then(|value| value.get("summary"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		problem_lines: diagnostic
			.and_then(|value| value.get("problem_lines"))
			.and_then(Value::as_array)
			.into_iter()
			.flatten()
			.filter_map(Value::as_str)
			.map(str::to_owned)
			.collect(),
		output_excerpt: diagnostic
			.and_then(|value| value.get("output_excerpt"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		output_truncated: diagnostic
			.and_then(|value| value.get("output_truncated"))
			.and_then(Value::as_bool),
	})
}
