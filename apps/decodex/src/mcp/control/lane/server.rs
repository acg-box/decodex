use serde_json::Value;

use crate::{
	mcp::{
		self, DEFAULT_MCP_STATUS_LIMIT, McpCapabilityProfile, McpServer, TOOL_LANE_CONTROL,
		control::lane::{args, args::LaneControlToolArgs, preconditions, results},
		observability,
	},
	orchestrator::{self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, McpLaneSteerRequest},
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

	fn call_lane_control_inspect_tool(
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

	fn call_lane_control_interrupt_tool(
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

	fn call_lane_control_steer_tool(
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
