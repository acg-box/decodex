use serde_json::{self, Value};

use crate::{
	agent::app_server::activity::{
		payload,
		protocol::{detail, status},
	},
	state::ProtocolActivityEventSummary,
};

pub(in crate::agent::app_server::activity::protocol) fn protocol_activity_event(
	event_type: &str,
	payload: &str,
) -> ProtocolActivityEventSummary {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	ProtocolActivityEventSummary {
		event_type: event_type.to_owned(),
		category: protocol_activity_category(event_type).to_owned(),
		detail: protocol_activity_detail(event_type, payload_value.as_ref()),
	}
}

pub(in crate::agent::app_server::activity::protocol) fn protocol_activity_category(
	event_type: &str,
) -> &'static str {
	let normalized = event_type.to_ascii_lowercase();

	if normalized.starts_with("turn/") {
		return "turn";
	}
	if normalized.contains("plan") {
		return "plan";
	}
	if normalized.contains("diff") || normalized.contains("filechange") {
		return "diff";
	}
	if normalized.contains("command")
		&& (normalized.contains("output") || normalized.contains("delta"))
	{
		return "command_output";
	}
	if normalized.contains("ratelimit") || normalized.contains("rate_limit") {
		return "rate_limit";
	}
	if normalized.starts_with("account/") {
		return "account";
	}
	if normalized == "deprecationnotice" {
		return "deprecation";
	}
	if normalized == "warning" || normalized == "configwarning" || normalized == "guardianwarning" {
		return "warning";
	}
	if normalized.starts_with("model/") {
		return "model";
	}
	if normalized.contains("tokenusage") {
		return "token_usage";
	}
	if normalized.contains("reasoning") {
		return "reasoning";
	}
	if normalized == "item/tool/call/failure" {
		return "protocol_error";
	}
	if normalized.ends_with("/response") || normalized == "json-rpc/error/response" {
		return "server_request_resolution";
	}
	if normalized.starts_with("item/") {
		return "item";
	}
	if normalized == "thread/status/changed" {
		return "thread";
	}
	if normalized == "error" || normalized.contains("error") {
		return "protocol_error";
	}

	"protocol"
}

fn protocol_activity_detail(event_type: &str, payload_value: Option<&Value>) -> Option<String> {
	let normalized = event_type.to_ascii_lowercase();

	if matches!(event_type, "thread/goal/set" | "thread/goal/get" | "thread/goal/updated") {
		return detail::phase_goal_activity_detail(payload_value);
	}
	if event_type == "thread/status/changed" {
		return payload_value.and_then(|value| {
			payload::string_at_paths(value, &[&["params", "status", "type"], &["status", "type"]])
		});
	}
	if event_type == "turn/steer" {
		return detail::protocol_steer_detail(payload_value);
	}
	if event_type.starts_with("turn/") {
		return status::protocol_turn_status_from_value(event_type, payload_value)
			.or_else(|| Some(String::from("running")));
	}
	if event_type == "item/tool/call" {
		return payload_value.and_then(payload::extract_tool_name);
	}
	if event_type == "item/tool/call/failure" {
		return payload_value.and_then(|value| {
			payload::string_at_paths(value, &[&["failureClass"], &["failure_class"]])
				.or_else(|| payload::string_at_paths(value, &[&["tool"]]))
		});
	}
	if event_type == "item/completed" {
		return payload_value.and_then(|value| {
			payload::string_at_paths(value, &[&["params", "item", "type"], &["item", "type"]])
		});
	}
	if event_type.starts_with("account/") {
		return detail::protocol_account_detail(payload_value);
	}
	if normalized == "deprecationnotice"
		|| normalized == "warning"
		|| normalized == "configwarning"
		|| normalized == "guardianwarning"
	{
		return detail::warning_or_deprecation_detail(payload_value);
	}
	if event_type == "model/rerouted" {
		return detail::model_rerouted_detail(payload_value);
	}
	if event_type == "model/verification" {
		return detail::model_verification_detail(payload_value);
	}
	if normalized.contains("tokenusage") {
		return detail::token_usage_detail(payload_value);
	}
	if normalized.contains("reasoning") {
		return payload_value.and_then(|value| {
			payload::string_at_paths(
				value,
				&[
					&["params", "text"],
					&["text"],
					&["params", "summary"],
					&["summary"],
					&["params", "part", "text"],
					&["part", "text"],
				],
			)
		});
	}
	if event_type == "error" {
		return payload_value
			.and_then(|value| {
				payload::string_at_paths(
					value,
					&[&["params", "error", "codexErrorInfo"], &["error", "codexErrorInfo"]],
				)
			})
			.or_else(|| Some(String::from("error")));
	}

	None
}
