use crate::mcp::{self, McpCapabilityProfile, McpTool, tool_schemas, tools::entry};

pub(super) fn mcp_operator_tools() -> Vec<McpTool> {
	vec![
		entry::mcp_tool_entry(
			McpCapabilityProfile::Operate,
			mcp::TOOL_LANE_CONTROL,
			"Decodex Lane Control",
			"Inspect a lane or request guarded soft lane-control actions with explicit authority.",
			tool_schemas::lane_control_tool_input_schema(),
			tool_schemas::lane_control_tool_output_schema(),
			false,
		),
		entry::mcp_tool_entry(
			McpCapabilityProfile::Admin,
			mcp::TOOL_PROJECT_CONTROL,
			"Decodex Project Control",
			"Pause or resume future project dispatch through the registered project enablement guard.",
			tool_schemas::project_control_tool_input_schema(),
			tool_schemas::project_control_tool_output_schema(),
			false,
		),
	]
}
