use std::time::Duration;

use serde_json::{self, Value};

use crate::state;

use super::{
	super::{MODEL_EXECUTION_IDLE_TIMEOUT, interactive_flag_for_request},
	CHILD_BUCKET_MODEL, CHILD_BUCKET_PROTOCOL, RECENT_PROTOCOL_ACTIVITY_LIMIT,
	WAITING_REASON_MODEL_EXECUTION,
	payload::{
		extract_tool_name, find_string_field, json_number_to_i64, string_at_paths, value_at_paths,
	},
};

pub(in crate::agent::app_server) struct ProtocolActivityAccumulator {
	pub(in crate::agent::app_server) summary: state::ProtocolActivitySummary,
}
impl ProtocolActivityAccumulator {
	pub(in crate::agent::app_server) fn new() -> Self {
		Self { summary: state::ProtocolActivitySummary::default() }
	}

	pub(in crate::agent::app_server) fn record(
		&mut self,
		event_type: &str,
		payload: &str,
		child_activity: &state::ChildAgentActivitySummary,
	) -> state::ProtocolActivitySummary {
		self.summary.recent_events.push(protocol_activity_event(event_type, payload));

		if self.summary.recent_events.len() > RECENT_PROTOCOL_ACTIVITY_LIMIT {
			let remove_count =
				self.summary.recent_events.len().saturating_sub(RECENT_PROTOCOL_ACTIVITY_LIMIT);

			self.summary.recent_events.drain(0..remove_count);
		}

		if let Some(turn_status) = protocol_turn_status_from_payload(event_type, payload) {
			self.summary.turn_status = Some(turn_status);
		}
		if let Some(waiting_reason) = protocol_waiting_reason(event_type, payload, child_activity) {
			self.summary.waiting_reason = Some(waiting_reason);
		}
		if let Some(rate_limit_status) = protocol_rate_limit_status(event_type, payload) {
			self.summary.rate_limit_status = Some(rate_limit_status);
		}

		self.summary.clone()
	}
}

pub(crate) fn protocol_activity_idle_timeout(
	protocol_activity: Option<&state::ProtocolActivitySummary>,
	base_timeout: Duration,
) -> Duration {
	if protocol_activity.is_some_and(running_model_execution_protocol_activity) {
		return base_timeout.max(MODEL_EXECUTION_IDLE_TIMEOUT);
	}

	base_timeout
}

fn running_model_execution_protocol_activity(
	protocol_activity: &state::ProtocolActivitySummary,
) -> bool {
	protocol_activity.turn_status.as_deref() == Some("running")
		&& protocol_activity.waiting_reason.as_deref() == Some(WAITING_REASON_MODEL_EXECUTION)
}

fn protocol_activity_event(event_type: &str, payload: &str) -> state::ProtocolActivityEventSummary {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	state::ProtocolActivityEventSummary {
		event_type: event_type.to_owned(),
		category: protocol_activity_category(event_type).to_owned(),
		detail: protocol_activity_detail(event_type, payload_value.as_ref()),
	}
}

fn protocol_activity_category(event_type: &str) -> &'static str {
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
		return phase_goal_activity_detail(payload_value);
	}
	if event_type == "thread/status/changed" {
		return payload_value.and_then(|value| {
			string_at_paths(value, &[&["params", "status", "type"], &["status", "type"]])
		});
	}
	if event_type == "turn/steer" {
		return protocol_steer_detail(payload_value);
	}
	if event_type.starts_with("turn/") {
		return protocol_turn_status_from_value(event_type, payload_value)
			.or_else(|| Some(String::from("running")));
	}
	if event_type == "item/tool/call" {
		return payload_value.and_then(extract_tool_name);
	}
	if event_type == "item/tool/call/failure" {
		return payload_value.and_then(|value| {
			string_at_paths(value, &[&["failureClass"], &["failure_class"]])
				.or_else(|| string_at_paths(value, &[&["tool"]]))
		});
	}
	if event_type == "item/completed" {
		return payload_value.and_then(|value| {
			string_at_paths(value, &[&["params", "item", "type"], &["item", "type"]])
		});
	}
	if event_type.starts_with("account/") {
		return protocol_account_detail(payload_value);
	}
	if normalized == "deprecationnotice"
		|| normalized == "warning"
		|| normalized == "configwarning"
		|| normalized == "guardianwarning"
	{
		return warning_or_deprecation_detail(payload_value);
	}
	if event_type == "model/rerouted" {
		return model_rerouted_detail(payload_value);
	}
	if event_type == "model/verification" {
		return model_verification_detail(payload_value);
	}
	if normalized.contains("tokenusage") {
		return token_usage_detail(payload_value);
	}
	if normalized.contains("reasoning") {
		return payload_value.and_then(|value| {
			string_at_paths(
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
				string_at_paths(
					value,
					&[&["params", "error", "codexErrorInfo"], &["error", "codexErrorInfo"]],
				)
			})
			.or_else(|| Some(String::from("error")));
	}

	None
}

fn phase_goal_activity_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let status = string_at_paths(
		value,
		&[&["payload", "status"], &["params", "goal", "status"], &["goal", "status"], &["status"]],
	)?;
	let phase = string_at_paths(value, &[&["phase"], &["payload", "phase"]]);

	Some(match phase {
		Some(phase) => format!("{phase}/{status}"),
		None => status,
	})
}

fn protocol_steer_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let outcome =
		string_at_paths(value, &[&["outcome"]]).unwrap_or_else(|| String::from("unknown"));
	let expected_turn_id = string_at_paths(value, &[&["expectedTurnId"], &["expected_turn_id"]])
		.unwrap_or_else(|| String::from("unknown"));
	let response_turn_id = string_at_paths(value, &[&["responseTurnId"], &["response_turn_id"]])
		.unwrap_or_else(|| String::from("none"));

	Some(format!("{outcome}: expected={expected_turn_id}, response={response_turn_id}"))
}

fn warning_or_deprecation_detail(payload_value: Option<&Value>) -> Option<String> {
	payload_value.and_then(|value| {
		string_at_paths(
			value,
			&[
				&["params", "summary"],
				&["summary"],
				&["params", "message"],
				&["message"],
				&["params", "details"],
				&["details"],
			],
		)
	})
}

fn model_rerouted_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let from_model = string_at_paths(value, &[&["params", "fromModel"], &["fromModel"]])?;
	let to_model = string_at_paths(value, &[&["params", "toModel"], &["toModel"]])?;
	let reason = string_at_paths(value, &[&["params", "reason"], &["reason"]]);

	Some(match reason {
		Some(reason) => format!("{from_model}->{to_model}/{reason}"),
		None => format!("{from_model}->{to_model}"),
	})
}

fn model_verification_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let verifications = value_at_paths(value, &[&["params", "verifications"], &["verifications"]])?;
	let verification_count = verifications.as_array()?.len();

	Some(format!("{verification_count} verification(s)"))
}

fn token_usage_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let input_tokens = value_at_paths(
		value,
		&[
			&["params", "tokenUsage", "total", "inputTokens"],
			&["tokenUsage", "total", "inputTokens"],
		],
	)
	.and_then(json_number_to_i64);
	let output_tokens = value_at_paths(
		value,
		&[
			&["params", "tokenUsage", "total", "outputTokens"],
			&["tokenUsage", "total", "outputTokens"],
		],
	)
	.and_then(json_number_to_i64);

	match (input_tokens, output_tokens) {
		(Some(input_tokens), Some(output_tokens)) =>
			Some(format!("input={input_tokens}, output={output_tokens}")),
		(Some(input_tokens), None) => Some(format!("input={input_tokens}")),
		(None, Some(output_tokens)) => Some(format!("output={output_tokens}")),
		(None, None) => None,
	}
}

fn protocol_account_detail(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let plan = string_at_paths(
		value,
		&[
			&["params", "planType"],
			&["params", "chatgptPlanType"],
			&["params", "rateLimits", "planType"],
			&["planType"],
			&["chatgptPlanType"],
			&["rateLimits", "planType"],
		],
	);
	let status = string_at_paths(
		value,
		&[
			&["params", "status"],
			&["params", "refreshStatus"],
			&["params", "rateLimits", "rateLimitReachedType"],
			&["status"],
			&["refreshStatus"],
			&["rateLimits", "rateLimitReachedType"],
		],
	);

	match (plan, status) {
		(Some(plan), Some(status)) => Some(format!("{plan}/{status}")),
		(Some(plan), None) => Some(plan),
		(None, Some(status)) => Some(status),
		(None, None) => None,
	}
}

fn protocol_turn_status_from_payload(event_type: &str, payload: &str) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	protocol_turn_status_from_value(event_type, payload_value.as_ref())
}

fn protocol_turn_status_from_value(
	event_type: &str,
	payload_value: Option<&Value>,
) -> Option<String> {
	match event_type {
		"turn/started" => Some(String::from("running")),
		"turn/completed" => payload_value
			.and_then(|value| {
				string_at_paths(value, &[&["params", "turn", "status"], &["turn", "status"]])
			})
			.or_else(|| Some(String::from("completed"))),
		_ => None,
	}
}

fn protocol_waiting_reason(
	event_type: &str,
	payload: &str,
	child_activity: &state::ChildAgentActivitySummary,
) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok();

	if event_type == "thread/status/changed"
		&& let Some(reason) = thread_status_waiting_reason(payload_value.as_ref())
	{
		return Some(reason);
	}
	if interactive_flag_for_request(event_type).is_some() {
		return Some(String::from("approval_or_user_input"));
	}
	if event_type == "item/tool/call" {
		return Some(String::from("tool_execution"));
	}
	if protocol_activity_category(event_type) == "command_output" {
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

fn thread_status_waiting_reason(payload_value: Option<&Value>) -> Option<String> {
	let value = payload_value?;
	let flags =
		value_at_paths(value, &[&["params", "status", "activeFlags"], &["status", "activeFlags"]])?;
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

fn protocol_rate_limit_status(_event_type: &str, payload: &str) -> Option<String> {
	let payload_value = serde_json::from_str::<Value>(payload).ok()?;

	find_string_field(&payload_value, &["rateLimitReachedType", "rate_limit_reached_type"]).or_else(
		|| {
			find_string_field(&payload_value, &["codexErrorInfo", "codex_error_info"])
				.filter(|value| value.to_ascii_lowercase().contains("limit"))
		},
	)
}
