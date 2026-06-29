use serde_json::Value;

use crate::mcp::{
	self, McpCapabilityProfile, McpServer, TOOL_LANE_CONTROL,
	control::lane::{args::LaneControlToolArgs, results},
};

impl McpServer {
	pub(in crate::mcp) fn call_lane_control_tool(
		&self,
		arguments: Value,
		profile: McpCapabilityProfile,
	) -> Value {
		let params = match serde_json::from_value::<LaneControlToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_LANE_CONTROL,
					"`action` is required and must be one of inspect, interrupt, steer, manual_attention, or retained_resume.",
				);
			},
		};

		if !matches!(
			params.action.as_str(),
			"inspect" | "interrupt" | "steer" | "manual_attention" | "retained_resume"
		) {
			return mcp::invalid_tool_arguments(
				TOOL_LANE_CONTROL,
				"`action` must be one of inspect, interrupt, steer, manual_attention, or retained_resume.",
			);
		}

		match params.action.as_str() {
			"inspect" => self.call_lane_control_inspect_tool(&params, profile),
			"interrupt" => self.call_lane_control_interrupt_tool(&params, profile),
			"steer" => self.call_lane_control_steer_tool(&params, profile),
			"manual_attention" => results::lane_control_refusal_result(
				&params,
				profile,
				"tracker_terminal_path_required",
				"MCP does not synthesize manual attention. Use the issue-scoped tracker terminal path so Decodex can validate the public blocker and terminal finalize state.",
			),
			"retained_resume" => results::lane_control_refusal_result(
				&params,
				profile,
				"runtime_lifecycle_required",
				"Retained resume is owned by the Decodex runtime lifecycle. Use the normal retained-lane dispatch path instead of an MCP shortcut.",
			),
			_ => unreachable!("lane-control action was validated above"),
		}
	}
}
