use serde_json::Value;

use crate::mcp::{
	self, McpCapabilityProfile,
	control::lane::{args::LaneControlToolArgs, preconditions},
	observability,
};

pub(super) fn lane_control_refusal_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	reason: &str,
	message: impl Into<String>,
) -> Value {
	mcp::tool_refusal_value(lane_control_result_value(
		params,
		profile,
		"refused",
		reason,
		message,
		serde_json::json!({}),
	))
}

pub(super) fn lane_control_interrupt_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	report: Value,
) -> Value {
	let soft = report.get("softInterrupt").unwrap_or(&Value::Null);
	let hard = report.get("hardInterrupt").unwrap_or(&Value::Null);
	let status =
		if hard.is_object() && hard.get("status").and_then(Value::as_str) != Some("unavailable") {
			"ok"
		} else {
			match soft.get("status").and_then(Value::as_str) {
				Some("delivered") => "ok",
				Some("pending") => "queued",
				_ => "refused",
			}
		};
	let reason =
		report.get("classification").and_then(Value::as_str).unwrap_or("lane_interrupt_result");
	let result = serde_json::json!({
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issue": report.get("issue").cloned().unwrap_or(Value::Null),
		"issueId": report.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": report.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": report.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"force": report.get("force").cloned().unwrap_or(Value::Bool(false)),
		"classification": report.get("classification").cloned().unwrap_or(Value::Null),
		"softInterrupt": {
			"attempted": soft.get("attempted").cloned().unwrap_or(Value::Bool(false)),
			"available": soft.get("available").cloned().unwrap_or(Value::Bool(false)),
			"status": soft.get("status").cloned().unwrap_or(Value::Null),
			"classification": soft.get("classification").cloned().unwrap_or(Value::Null),
			"method": soft.get("method").cloned().unwrap_or(Value::Null),
			"requestId": soft.get("requestId").cloned().unwrap_or(Value::Null),
			"message": soft.get("message").cloned().unwrap_or(Value::Null),
			"errorClass": soft.get("errorClass").cloned().unwrap_or(Value::Null)
		},
		"hardInterrupt": if hard.is_object() {
			serde_json::json!({
				"attempted": hard.get("attempted").cloned().unwrap_or(Value::Bool(false)),
				"status": hard.get("status").cloned().unwrap_or(Value::Null),
				"classification": hard.get("classification").cloned().unwrap_or(Value::Null),
				"signals": hard.get("signals").cloned().unwrap_or_else(|| serde_json::json!([])),
				"message": hard.get("message").cloned().unwrap_or(Value::Null),
				"errorClass": hard.get("errorClass").cloned().unwrap_or(Value::Null)
			})
		} else {
			Value::Null
		},
		"nextAction": report.get("nextAction").cloned().unwrap_or(Value::Null)
	});
	let value = lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane interrupt completed through the existing lane-control guard path.",
		result,
	);

	if status == "refused" { mcp::tool_refusal_value(value) } else { mcp::tool_success(value) }
}

pub(super) fn lane_control_steer_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	report: Value,
) -> Value {
	let outcome = report.get("outcome").and_then(Value::as_str).unwrap_or("unknown");
	let delivery_status = report.get("deliveryStatus").and_then(Value::as_str).unwrap_or("unknown");
	let failure_class = report.get("failureClass").and_then(Value::as_str);
	let status = if delivery_status == "queued" {
		"queued"
	} else if matches!(outcome, "rejected" | "failed" | "timed_out" | "fallback") {
		"refused"
	} else {
		"ok"
	};
	let reason = failure_class
		.or_else(|| report.get("reason").and_then(Value::as_str))
		.unwrap_or("lane_steer_result");
	let result = serde_json::json!({
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issueId": report.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": report.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": report.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"expectedTurnId": report.get("expectedTurnId").cloned().unwrap_or(Value::Null),
		"currentTurnId": report.get("currentTurnId").cloned().unwrap_or(Value::Null),
		"responseTurnId": report.get("responseTurnId").cloned().unwrap_or(Value::Null),
		"auditRecordId": report.get("auditRecordId").cloned().unwrap_or(Value::Null),
		"requestId": report.get("requestId").cloned().unwrap_or(Value::Null),
		"outcome": report.get("outcome").cloned().unwrap_or(Value::Null),
		"reason": report.get("reason").cloned().unwrap_or(Value::Null),
		"failureClass": report.get("failureClass").cloned().unwrap_or(Value::Null),
		"deliveryStatus": report.get("deliveryStatus").cloned().unwrap_or(Value::Null),
		"messageByteCount": report.get("messageByteCount").cloned().unwrap_or(Value::Null),
		"messageLineCount": report.get("messageLineCount").cloned().unwrap_or(Value::Null)
	});
	let value = lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane steer returned without exposing the original steer message.",
		result,
	);

	if status == "refused" { mcp::tool_refusal_value(value) } else { mcp::tool_success(value) }
}

fn lane_control_result_value(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	status: &str,
	reason: &str,
	message: impl Into<String>,
	result: Value,
) -> Value {
	let mut value = serde_json::json!({
		"schema": "decodex.mcp.lane_control_result/1",
		"status": status,
		"reason": reason,
		"message": message.into(),
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"project_id": params.project_id.as_deref(),
		"issue": params.issue.as_deref(),
		"run_id": params.run_id.as_deref(),
		"preconditions": preconditions::lane_control_preconditions(params),
		"result": result
	});

	observability::sanitize_mcp_observability_value(&mut value);

	value
}
