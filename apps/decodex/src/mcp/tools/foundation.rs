use crate::mcp::{self, McpCapabilityProfile, McpTool, tool_schemas, tools::entry};

pub(super) fn mcp_foundation_tools() -> Vec<McpTool> {
	vec![
		entry::mcp_tool_entry(
			McpCapabilityProfile::Observe,
			mcp::TOOL_OBSERVE,
			"Decodex Observe",
			"Read public-safe local Decodex runtime observability without private evidence payloads.",
			tool_schemas::observe_tool_input_schema(),
			tool_schemas::observe_tool_output_schema(),
			true,
		),
		entry::mcp_tool_entry(
			McpCapabilityProfile::Plan,
			mcp::TOOL_PLAN,
			"Decodex Plan",
			"Return the Decodex prompt/resource route for a requested workflow intent.",
			tool_schemas::plan_tool_input_schema(),
			tool_schemas::plan_tool_output_schema(),
			true,
		),
		entry::mcp_tool_entry(
			McpCapabilityProfile::Plan,
			mcp::TOOL_INTAKE_GOAL,
			"Decodex Goal Intake",
			"Dry-run or explicitly apply promoted-goal Program Intake through Decodex authority gates.",
			tool_schemas::intake_goal_tool_input_schema(),
			tool_schemas::intake_goal_tool_output_schema(),
			false,
		),
	]
}
