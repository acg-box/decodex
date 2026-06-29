use serde_json::Value;

use crate::mcp::{
	self, McpCapabilityProfile,
	control::lane::{args::LaneControlToolArgs, results::base},
};

pub(in crate::mcp::control::lane) fn lane_control_steer_result(
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
	let value = base::lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane steer returned without exposing the original steer message.",
		result,
	);

	if status == "refused" { mcp::tool_refusal_value(value) } else { mcp::tool_success(value) }
}
