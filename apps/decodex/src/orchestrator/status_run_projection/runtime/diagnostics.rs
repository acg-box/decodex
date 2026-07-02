use std::time::Duration;

use crate::orchestrator::{
	self, CONTINUATION_PENDING_RUN_STATUS, OperatorRunTiming, PrivateExecutionEvent,
	ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_IDLE,
	RUN_OPERATION_WAITING_EXTERNAL, TERMINAL_GUARDED_RUN_STATUS, Value, state,
};

pub(in crate::orchestrator) fn classify_operator_run_operation(
	phase: &str,
	marker_current_operation: Option<&str>,
) -> String {
	match phase {
		"retry_backoff" | "waiting_continuation" => String::from(RUN_OPERATION_WAITING_EXTERNAL),
		"completed" | "failed" => String::from(RUN_OPERATION_IDLE),
		"stalled" => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_IDLE)),
		"executing" => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_AGENT_RUN)),
		_ => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_IDLE)),
	}
}

pub(in crate::orchestrator) fn operator_run_is_suspected_stall(
	phase: &str,
	last_progress_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
	idle_timeout: Duration,
) -> bool {
	if phase != "executing" {
		return false;
	}

	last_progress_unix_epoch
		.and_then(|last_progress| {
			orchestrator::observed_idle_duration(last_progress, now_unix_epoch)
		})
		.is_some_and(|idle_for| {
			idle_for >= suspected_operator_run_stall_threshold(idle_timeout)
				&& idle_for < idle_timeout
		})
}

pub(in crate::orchestrator) fn suspected_operator_run_stall_threshold(
	idle_timeout: Duration,
) -> Duration {
	Duration::from_secs((idle_timeout.as_secs() / 2).max(1))
}

pub(in crate::orchestrator) fn operator_run_progress_diagnostic(
	phase: &str,
	timing: &OperatorRunTiming,
	protocol_activity: Option<&ProtocolActivitySummary>,
	private_events: &[PrivateExecutionEvent],
	now_unix_epoch: i64,
	idle_timeout: Duration,
) -> Option<String> {
	if let Some(repo_gate_diagnostic) =
		operator_latest_repo_gate_failure_progress_diagnostic(private_events)
	{
		return Some(repo_gate_diagnostic);
	}

	if phase != "executing" {
		return None;
	}

	let protocol_activity = protocol_activity?;

	if protocol_activity.waiting_reason.as_deref() != Some("model_execution")
		|| !protocol_activity_is_non_work_only(protocol_activity)
	{
		return None;
	}

	let protocol_idle = timing.last_protocol_activity_unix_epoch.and_then(|last_protocol| {
		orchestrator::observed_idle_duration(last_protocol, now_unix_epoch)
	})?;

	if protocol_idle >= idle_timeout {
		return None;
	}

	let progress_is_stale = timing
		.last_progress_unix_epoch
		.and_then(|last_progress| {
			orchestrator::observed_idle_duration(last_progress, now_unix_epoch)
		})
		.is_none_or(|idle_for| idle_for >= suspected_operator_run_stall_threshold(idle_timeout));

	progress_is_stale.then(|| String::from("protocol_only_activity"))
}

pub(in crate::orchestrator) fn operator_latest_repo_gate_failure_progress_diagnostic(
	private_events: &[PrivateExecutionEvent],
) -> Option<String> {
	private_events
		.iter()
		.rev()
		.find(|event| event.event_type() == "phase_goal_transition")
		.and_then(operator_repo_gate_failure_progress_diagnostic)
}

pub(in crate::orchestrator) fn operator_repo_gate_failure_progress_diagnostic(
	event: &PrivateExecutionEvent,
) -> Option<String> {
	if event.event_type() != "phase_goal_transition" {
		return None;
	}

	let transition_payload = event.payload().get("payload")?;
	let error_class = transition_payload.get("errorClass")?.as_str()?;

	if !error_class.starts_with("repo_gate_") {
		return None;
	}

	let failed_command = transition_payload
		.get("repoGateFailure")
		.and_then(|diagnostic| diagnostic.get("failed_command"))
		.and_then(Value::as_str)
		.unwrap_or("inspect_private_evidence");

	Some(format!("repo_gate_failure:{error_class}; failed_command:{failed_command}"))
}

pub(in crate::orchestrator) fn protocol_activity_is_non_work_only(
	protocol_activity: &ProtocolActivitySummary,
) -> bool {
	!protocol_activity.recent_events.is_empty()
		&& protocol_activity
			.recent_events
			.iter()
			.all(|event| !state::protocol_event_counts_as_work_progress(&event.event_type))
}

pub(in crate::orchestrator) fn visible_operator_run_retry_schedule(
	status: &str,
	retry_kind: Option<&str>,
	retry_ready_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> (Option<String>, Option<i64>) {
	let Some(retry_ready_at_unix_epoch) = retry_ready_at_unix_epoch else {
		return (None, None);
	};

	if matches!(status, "starting" | "running") || retry_ready_at_unix_epoch <= now_unix_epoch {
		return (None, None);
	}

	(retry_kind.map(str::to_owned), Some(retry_ready_at_unix_epoch))
}

pub(in crate::orchestrator) fn classify_operator_run_phase(
	status: &str,
	retry_kind: Option<&str>,
	retry_ready_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> (String, Option<String>) {
	if status == "stalled" {
		return (String::from("stalled"), Some(String::from("app_server_idle_timeout")));
	}

	if let Some(retry_ready_at_unix_epoch) = retry_ready_at_unix_epoch
		&& retry_ready_at_unix_epoch > now_unix_epoch
	{
		return (
			String::from("retry_backoff"),
			Some(match retry_kind {
				Some("continuation") => String::from("continuation_retry"),
				Some("failure") => String::from("failure_retry"),
				Some(other) => other.to_owned(),
				None => String::from("scheduled_retry"),
			}),
		);
	}

	match status {
		"starting" | "running" => (String::from("executing"), None),
		CONTINUATION_PENDING_RUN_STATUS => {
			(String::from("waiting_continuation"), Some(String::from("turn_boundary")))
		},
		"succeeded" => (String::from("completed"), None),
		"failed" | "interrupted" | TERMINAL_GUARDED_RUN_STATUS => (String::from("failed"), None),
		other => (other.to_owned(), None),
	}
}
