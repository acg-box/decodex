use serde_json::Value;

use crate::{recovery::evidence::json, state::PrivateExecutionEvent};

pub(in crate::recovery::evidence::stale_active) fn stale_active_private_event_is_stale_runtime_marker(
	event: &PrivateExecutionEvent,
) -> bool {
	matches!(event.event_type(), "control_channel_published" | "phase_goal_set")
}

pub(in crate::recovery::evidence::stale_active) fn stale_active_private_event_is_probing_checkpoint(
	event: &PrivateExecutionEvent,
) -> bool {
	if event.event_type() != "progress_checkpoint" {
		return false;
	}

	let payload = event.payload();

	payload.get("phase").and_then(Value::as_str) == Some("probing")
		&& json::string_is_missing_or_empty(payload.get("pr_url"))
		&& json::array_is_missing_or_empty(payload.get("verification"))
}

pub(in crate::recovery::evidence::stale_active) fn stale_active_event_is_phase_goal_failure_telemetry(
	event: &PrivateExecutionEvent,
) -> bool {
	if !matches!(event.event_type(), "phase_goal_recovery" | "phase_goal_recovery_blocked") {
		return false;
	}

	let payload = event.payload();
	let source_error_class = payload.pointer("/payload/sourceErrorClass").and_then(Value::as_str);

	payload.get("schema").and_then(Value::as_str) == Some("decodex.phase_goal_signal/1")
		&& payload.get("phase").and_then(Value::as_str) == Some("implement_to_validation_ready")
		&& matches!(
			payload.get("signal").and_then(Value::as_str),
			Some("phase_goal_recovered" | "continuation_budget_exhausted")
		) && source_error_class.is_some_and(|error_class| error_class.starts_with("app_server_"))
}
