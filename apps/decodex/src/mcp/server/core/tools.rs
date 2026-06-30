use serde_json::Value;

use super::McpServer;
use crate::{
	mcp::{
		self, McpError, TOOL_AUTONOMY_ACCEPT_OBJECTIVE, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		TOOL_AUTONOMY_COMPILE_PROPOSAL, TOOL_AUTONOMY_DRAFT_OBJECTIVE,
		TOOL_AUTONOMY_REQUEST_PROMOTION, TOOL_AUTONOMY_SUBMIT_SIGNAL, TOOL_INTAKE_GOAL,
		TOOL_LANE_CONTROL, TOOL_OBSERVE, TOOL_PLAN, TOOL_PROJECT_CONTROL, TOOL_RESEARCH_COMPILE,
		TOOL_RESEARCH_PROMOTE, planning, server::protocol::CallToolParams, tools,
	},
	prelude::Result,
};

impl McpServer {
	pub(super) fn list_tools(&self) -> Value {
		let tools = tools::mcp_tools()
			.into_iter()
			.filter(|tool| self.capability_profile.allows(tool.required_profile))
			.map(|tool| tool.value)
			.collect::<Vec<_>>();

		serde_json::json!({ "tools": tools })
	}

	pub(super) fn call_tool(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<CallToolParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let arguments = params.arguments.unwrap_or_else(|| serde_json::json!({}));
		let Some(required_profile) = tools::tool_required_profile(&params.name) else {
			return Ok(mcp::tool_refusal(
				"unknown_tool",
				format!("Decodex MCP tool `{}` is not registered.", params.name),
			));
		};

		if !self.capability_profile.allows(required_profile) {
			return Ok(mcp::capability_profile_refusal(
				&params.name,
				self.capability_profile,
				required_profile,
			));
		}

		match params.name.as_str() {
			TOOL_OBSERVE => Ok(self.call_observe_tool(arguments)),
			TOOL_PLAN => Ok(planning::call_plan_tool(arguments)),
			TOOL_RESEARCH_COMPILE => Ok(self.call_research_compile_tool(arguments)),
			TOOL_RESEARCH_PROMOTE => Ok(self.call_research_promote_tool(arguments)),
			TOOL_INTAKE_GOAL => Ok(self.call_intake_goal_tool(arguments)),
			TOOL_AUTONOMY_DRAFT_OBJECTIVE => Ok(self.call_autonomy_draft_objective_tool(arguments)),
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE =>
				Ok(self.call_autonomy_accept_objective_tool(arguments)),
			TOOL_AUTONOMY_SUBMIT_SIGNAL => Ok(self.call_autonomy_submit_signal_tool(arguments)),
			TOOL_AUTONOMY_COMPILE_PROPOSAL =>
				Ok(self.call_autonomy_compile_proposal_tool(arguments)),
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL =>
				Ok(self.call_autonomy_challenge_proposal_tool(arguments)),
			TOOL_AUTONOMY_REQUEST_PROMOTION =>
				Ok(self.call_autonomy_request_promotion_tool(arguments)),
			TOOL_LANE_CONTROL => Ok(self.call_lane_control_tool(arguments, required_profile)),
			TOOL_PROJECT_CONTROL => Ok(self.call_project_control_tool(arguments, required_profile)),
			_ => Ok(mcp::tool_refusal("unknown_tool", "Decodex MCP tool is not registered.")),
		}
	}
}
