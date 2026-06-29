use serde_json::Value;

use crate::mcp::{
	self, McpCapabilityProfile,
	control::lane::{args::LaneControlToolArgs, results::base},
};

pub(in crate::mcp::control::lane) fn lane_control_interrupt_result(
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
	let value = base::lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane interrupt completed through the existing lane-control guard path.",
		result,
	);

	if status == "refused" { mcp::tool_refusal_value(value) } else { mcp::tool_success(value) }
}
