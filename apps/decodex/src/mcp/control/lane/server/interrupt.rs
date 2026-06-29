use serde_json::Value;

use crate::{
	mcp::{
		self, McpCapabilityProfile, McpServer,
		control::lane::{args, args::LaneControlToolArgs, results},
	},
	orchestrator,
};

impl McpServer {
	pub(in crate::mcp::control::lane::server) fn call_lane_control_interrupt_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = mcp::non_empty_string(params.issue.as_deref()) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control interrupt.",
			);
		};
		let Some(run_id) = mcp::non_empty_string(params.run_id.as_deref()) else {
			return results::lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_missing",
				"`runId` from lane-control inspect is required for interrupt.",
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
		if params.force.unwrap_or(false) && !authority.allow_hard_fallback {
			return results::lane_control_refusal_result(
				params,
				profile,
				"hard_fallback_authority_missing",
				"Hard interrupt fallback requires force=true and authority.allowHardFallback=true.",
			);
		}

		let report = match orchestrator::run_mcp_lane_interrupt(
			self.context.config_path.as_deref(),
			issue,
			run_id,
			params.force.unwrap_or(false),
			Some(authority.reason),
			authority.source,
		) {
			Ok(report) => report,
			Err(error) => {
				return results::lane_control_refusal_result(
					params,
					profile,
					"lane_interrupt_unavailable",
					format!("Lane interrupt failed closed: {error}"),
				);
			},
		};

		results::lane_control_interrupt_result(params, profile, report)
	}
}
