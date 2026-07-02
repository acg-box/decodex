use serde_json::{self, Value};

use crate::{
	agent::app_server::{
		self,
		activity::{
			CHILD_BUCKET_MODEL, CHILD_BUCKET_PROTOCOL, WAITING_REASON_MODEL_EXECUTION, payload,
			protocol::event,
		},
	},
	state::{ChildAgentActivitySummary, ProtocolActivitySummary},
};

pub(in crate::agent::app_server::activity::protocol) fn running_model_execution_protocol_activity(
	protocol_activity: &ProtocolActivitySummary,
) -> bool {
	protocol_activity.turn_status.as_deref() == Some("running")
		&& protocol_activity.waiting_reason.as_deref() == Some(WAITING_REASON_MODEL_EXECUTION)
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_turn_status_from_payload(
	event_type: &str,
	payload: &str,
) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	protocol_turn_status_from_value(event_type, payload_value.as_ref())
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_turn_status_from_value(
	event_type: &str,
	payload_value: Option<&Value>,
) -> Option<String> {
	match event_type {
		"turn/started" => Some(String::from("running")),
		"turn/completed" => payload_value
			.and_then(|value| {
				payload::string_at_paths(
					value,
					&[&["params", "turn", "status"], &["turn", "status"]],
				)
			})
			.or_else(|| Some(String::from("completed"))),
		_ => None,
	}
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_waiting_reason(
	event_type: &str,
	payload: &str,
	child_activity: &ChildAgentActivitySummary,
) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	if event_type == "thread/status/changed"
		&& let Some(reason) = thread_status_waiting_reason(payload_value.as_ref())
	{
		return Some(reason);
	}
	if app_server::interactive_flag_for_request(event_type).is_some() {
		return Some(String::from("approval_or_user_input"));
	}
	if event_type == "item/tool/call" {
		return Some(String::from("tool_execution"));
	}
	if event::protocol_activity_category(event_type) == "command_output" {
		return Some(String::from("tool_execution"));
	}
	if matches!(event_type, "item/tool/call/response" | "item/completed" | "turn/started")
		|| event_type.ends_with("/delta")
		|| event_type.ends_with("/response")
	{
		return Some(String::from("model_execution"));
	}
	if event_type == "turn/completed" {
		return Some(String::from("turn_completed"));
	}

	if let Some(current_bucket) = child_activity.current_bucket.as_deref() {
		return Some(match current_bucket {
			CHILD_BUCKET_MODEL => String::from("model_execution"),
			CHILD_BUCKET_PROTOCOL => String::from("protocol_activity"),
			_ => String::from("tool_execution"),
		});
	}

	None
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_rate_limit_status(
	_event_type: &str,
	payload: &str,
) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok()?;

	payload::find_string_field(&payload_value, &["rateLimitReachedType", "rate_limit_reached_type"])
		.or_else(|| {
			payload::find_string_field(&payload_value, &["codexErrorInfo", "codex_error_info"])
				.filter(|value| value.to_ascii_lowercase().contains("limit"))
		})
}

fn thread_status_waiting_reason(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let flags = payload::value_at_paths(
		value,
		&[&["params", "status", "activeFlags"], &["status", "activeFlags"]],
	)?;
	let flags = flags.as_array()?;

	if flags
		.iter()
		.filter_map(Value::as_str)
		.any(|flag| matches!(flag, "waitingOnApproval" | "waitingOnUserInput"))
	{
		return Some(String::from("approval_or_user_input"));
	}

	None
}
