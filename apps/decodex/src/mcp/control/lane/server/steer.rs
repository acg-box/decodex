use serde_json::Value;

use crate::{
	mcp::{
		self, McpCapabilityProfile, McpServer,
		control::lane::{args, args::LaneControlToolArgs, results},
	},
	orchestrator::{self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, McpLaneSteerRequest},
};

impl McpServer {
	pub(in crate::mcp::control::lane::server) fn call_lane_control_steer_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = mcp::non_empty_string(params.issue.as_deref()) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control steer.",
			);
		};
		let Some(run_id) = mcp::non_empty_string(params.run_id.as_deref()) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_missing",
				"`runId` from lane-control inspect is required for steer.",
			);
		};
		let Some(expected_turn_id) = mcp::non_empty_string(params.expected_turn_id.as_deref())
		else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"expected_turn_id_required",
				"`expectedTurnId` from lane-control inspect is required for steer.",
			);
		};
		let Some(message) = mcp::non_empty_string(params.message.as_deref()) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"message_required",
				"`message` is required for steer and is never echoed in MCP results.",
			);
		};
		let Some(authority) = args::lane_control_authority(params) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Mutating lane-control calls require authority.reason, authority.source, and authority.inspectedRunId.",
			);
		};

		if authority.inspected_run_id != run_id {
			return results::lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_mismatch",
				"authority.inspectedRunId must match the requested runId.",
			);
		}
		if authority.expected_turn_id != Some(expected_turn_id) {
			return results::lane_control_refusal_result(
				params,
				profile,
				"expected_turn_authority_mismatch",
				"authority.expectedTurnId must match the requested expectedTurnId.",
			);
		}

		let report = match orchestrator::run_mcp_lane_steer(McpLaneSteerRequest {
			config_path: self.context.config_path.as_deref(),
			project_id: params
				.project_id
				.as_deref()
				.and_then(|project_id| mcp::non_empty_string(Some(project_id))),
			issue,
			run_id,
			expected_turn_id,
			message,
			source: authority.source,
			wait_timeout: DEFAULT_STEER_RESULT_WAIT_TIMEOUT,
		}) {
			Ok(report) => report,
			Err(error) => {
				return results::lane_control_refusal_result(
					params,
					profile,
					"lane_steer_unavailable",
					format!("Lane steer failed closed: {error}"),
				);
			},
		};

		results::lane_control_steer_result(params, profile, report)
	}
}
