use serde_json::Value;

use crate::mcp::{
	self, McpCapabilityProfile, control::project::args::ProjectControlToolArgs, observability,
};

pub(super) fn project_control_success_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
	result: Value,
) -> Value {
	mcp::tool_success(project_control_result_value(
		params,
		profile,
		project_id,
		"ok",
		params.action.as_str(),
		"Project control completed through the registered project enablement guard.",
		result,
	))
}

pub(super) fn project_control_refusal_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	reason: &str,
	message: impl Into<String>,
) -> Value {
	let project_id = params.project_id.as_deref().unwrap_or("");

	mcp::tool_refusal_value(project_control_result_value(
		params,
		profile,
		project_id,
		"refused",
		reason,
		message,
		serde_json::json!({}),
	))
}

fn project_control_result_value(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
	status: &str,
	reason: &str,
	message: impl Into<String>,
	result: Value,
) -> Value {
	let mut value = serde_json::json!({
		"schema": "decodex.mcp.project_control_result/1",
		"status": status,
		"reason": reason,
		"message": message.into(),
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"project_id": mcp::non_empty_string(Some(project_id)),
		"future_dispatch_only": true,
		"result": result
	});

	observability::sanitize_mcp_observability_value(&mut value);

	value
}
