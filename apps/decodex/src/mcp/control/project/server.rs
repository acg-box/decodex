use serde_json::{self, Value};

use crate::mcp::{
	self, McpCapabilityProfile, McpServer, TOOL_PROJECT_CONTROL,
	control::project::{args::ProjectControlToolArgs, results, status},
};

impl McpServer {
	pub(in crate::mcp) fn call_project_control_tool(
		&self,
		arguments: Value,
		profile: McpCapabilityProfile,
	) -> Value {
		let params = match serde_json::from_value::<ProjectControlToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_PROJECT_CONTROL,
					"`action` is required and must be one of status, pause, resume, or scan.",
				);
			},
		};

		if !matches!(params.action.as_str(), "status" | "pause" | "resume" | "scan") {
			return mcp::invalid_tool_arguments(
				TOOL_PROJECT_CONTROL,
				"`action` must be one of status, pause, resume, or scan.",
			);
		}

		let Some(project_id) = mcp::non_empty_string(params.project_id.as_deref())
			.or(self.context.project_id.as_deref())
		else {
			return results::project_control_refusal_result(
				&params,
				profile,
				"project_id_required",
				"`projectId` is required when the MCP gateway is not bound to one project config.",
			);
		};

		if let Some(context_project_id) = self.context.project_id.as_deref()
			&& context_project_id != project_id
		{
			return results::project_control_refusal_result(
				&params,
				profile,
				"project_mismatch",
				"The requested projectId does not match this MCP gateway context.",
			);
		}

		match params.action.as_str() {
			"status" => status::project_control_status_result(&params, profile, project_id),
			"scan" => results::project_control_refusal_result(
				&params,
				profile,
				"operator_control_loop_required",
				"Linear scan requests are queued by the Decodex operator control-plane loop; standalone MCP serve cannot enqueue that in-memory request.",
			),
			"pause" | "resume" => self.call_project_enablement_tool(&params, profile, project_id),
			_ => unreachable!("project-control action was validated above"),
		}
	}
}
