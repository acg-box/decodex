use crate::{recovery::evidence::json, state::PrivateExecutionEvent};

pub(in crate::recovery::evidence::stale_active) fn stale_active_private_event_is_no_diff_guardrail(
	event: &PrivateExecutionEvent,
) -> bool {
	if event.event_type() != "loop_guardrail_checkpoint" {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str)
		== Some("decodex.loop_guardrail_checkpoint/1")
		&& stale_active_no_diff_guardrail_source_is_startup_or_turn_failure(payload)
		&& payload.get("reason").and_then(serde_json::Value::as_str) == Some("no_effective_diff")
		&& stale_active_guardrail_details_have_no_delta(payload)
}

fn stale_active_no_diff_guardrail_source_is_startup_or_turn_failure(
	payload: &serde_json::Value,
) -> bool {
	let source_error_class = payload.get("source_error_class").and_then(serde_json::Value::as_str);

	matches!(
		source_error_class,
		Some("app_server_turn_failed" | "app_server_turn_missing_error_payload") | None
	)
}

fn stale_active_guardrail_details_have_no_delta(payload: &serde_json::Value) -> bool {
	let details = payload
		.get("details")
		.and_then(serde_json::Value::as_str)
		.and_then(|details| serde_json::from_str::<serde_json::Value>(details).ok());
	let details = details.as_ref().unwrap_or(payload);

	json::bool_is_false(details.get("branch_delta_present"))
		&& json::bool_is_false(details.get("effective_delta_present"))
}
