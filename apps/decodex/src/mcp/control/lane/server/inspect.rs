use serde_json::Value;

use crate::{
	mcp::{
		self, DEFAULT_MCP_STATUS_LIMIT, McpCapabilityProfile, McpServer,
		control::lane::{args::LaneControlToolArgs, preconditions, results},
		observability,
	},
	orchestrator,
};

impl McpServer {
	pub(in crate::mcp::control::lane::server) fn call_lane_control_inspect_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = mcp::non_empty_string(params.issue.as_deref()) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control inspect.",
			);
		};

		if let Some(project_id) = mcp::non_empty_string(params.project_id.as_deref())
			&& Some(project_id) != self.context.project_id.as_deref()
		{
			return results::lane_control_refusal_result(
				params,
				profile,
				"project_mismatch",
				"The requested projectId does not match this MCP gateway context.",
			);
		}

		let report = match orchestrator::build_mcp_lane_control_resource(
			self.context.config_path.as_deref(),
			Some(issue),
			params.run_id.as_deref().and_then(|run_id| mcp::non_empty_string(Some(run_id))),
			DEFAULT_MCP_STATUS_LIMIT,
		) {
			Ok(report) => report,
			Err(error) => {
				return results::lane_control_refusal_result(
					params,
					profile,
					"lane_inspect_unavailable",
					format!("Lane inspect failed closed: {error}"),
				);
			},
		};
		let mut result = serde_json::json!({
			"schema": "decodex.mcp.lane_control_result/1",
			"status": "ok",
			"reason": "inspect_complete",
			"message": "Inspect returned current lane-control preconditions for any later mutating request.",
			"capability_profile": profile.as_str(),
			"action": "inspect",
			"project_id": self.context.project_id.as_deref(),
			"issue": issue,
			"run_id": params.run_id.as_deref(),
			"preconditions": preconditions::lane_control_preconditions(params),
			"result": {
				"inspect": observability::mcp_public_lane_inspect_resource(report.clone()),
				"mutating_preconditions": preconditions::lane_control_mutating_preconditions(&report)
			}
		});

		observability::sanitize_mcp_observability_value(&mut result);

		mcp::tool_success(result)
	}
}
