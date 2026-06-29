use serde_json::Value;

use crate::mcp::{
	self, McpCapabilityProfile,
	control::lane::{args::LaneControlToolArgs, preconditions},
	observability,
};

pub(in crate::mcp::control::lane) fn lane_control_refusal_result(
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

pub(super) fn lane_control_result_value(
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
