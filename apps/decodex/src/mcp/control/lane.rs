use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	mcp::{
		DEFAULT_MCP_STATUS_LIMIT, McpCapabilityProfile, McpServer, TOOL_LANE_CONTROL,
		invalid_tool_arguments, non_empty_string,
		observability::{mcp_public_lane_inspect_resource, sanitize_mcp_observability_value},
		tool_refusal_value, tool_success,
	},
	orchestrator::{self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, McpLaneSteerRequest},
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaneControlToolArgs {
	action: String,
	project_id: Option<String>,
	issue: Option<String>,
	run_id: Option<String>,
	expected_turn_id: Option<String>,
	message: Option<String>,
	force: Option<bool>,
	authority: Option<LaneControlAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LaneControlAuthorityArgs {
	reason: Option<String>,
	source: Option<String>,
	inspected_run_id: Option<String>,
	expected_turn_id: Option<String>,
	allow_hard_fallback: Option<bool>,
}

struct LaneControlAuthority<'a> {
	reason: &'a str,
	source: &'a str,
	inspected_run_id: &'a str,
	expected_turn_id: Option<&'a str>,
	allow_hard_fallback: bool,
}

impl McpServer {
	pub(in crate::mcp) fn call_lane_control_tool(
		&self,
		arguments: Value,
		profile: McpCapabilityProfile,
	) -> Value {
		let params = match serde_json::from_value::<LaneControlToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_LANE_CONTROL,
					"`action` is required and must be one of inspect, interrupt, steer, manual_attention, or retained_resume.",
				);
			},
		};

		if !matches!(
			params.action.as_str(),
			"inspect" | "interrupt" | "steer" | "manual_attention" | "retained_resume"
		) {
			return invalid_tool_arguments(
				TOOL_LANE_CONTROL,
				"`action` must be one of inspect, interrupt, steer, manual_attention, or retained_resume.",
			);
		}

		match params.action.as_str() {
			"inspect" => self.call_lane_control_inspect_tool(&params, profile),
			"interrupt" => self.call_lane_control_interrupt_tool(&params, profile),
			"steer" => self.call_lane_control_steer_tool(&params, profile),
			"manual_attention" => lane_control_refusal_result(
				&params,
				profile,
				"tracker_terminal_path_required",
				"MCP does not synthesize manual attention. Use the issue-scoped tracker terminal path so Decodex can validate the public blocker and terminal finalize state.",
			),
			"retained_resume" => lane_control_refusal_result(
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
		let Some(issue) = non_empty_string(params.issue.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control inspect.",
			);
		};

		if let Some(project_id) = non_empty_string(params.project_id.as_deref())
			&& Some(project_id) != self.context.project_id.as_deref()
		{
			return lane_control_refusal_result(
				params,
				profile,
				"project_mismatch",
				"The requested projectId does not match this MCP gateway context.",
			);
		}

		let report = match orchestrator::build_mcp_lane_control_resource(
			self.context.config_path.as_deref(),
			Some(issue),
			params.run_id.as_deref().and_then(|run_id| non_empty_string(Some(run_id))),
			DEFAULT_MCP_STATUS_LIMIT,
		) {
			Ok(report) => report,
			Err(error) => {
				return lane_control_refusal_result(
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
			"preconditions": lane_control_preconditions(params),
			"result": {
				"inspect": mcp_public_lane_inspect_resource(report.clone()),
				"mutating_preconditions": lane_control_mutating_preconditions(&report)
			}
		});

		sanitize_mcp_observability_value(&mut result);

		tool_success(result)
	}

	fn call_lane_control_interrupt_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = non_empty_string(params.issue.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control interrupt.",
			);
		};
		let Some(run_id) = non_empty_string(params.run_id.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_missing",
				"`runId` from lane-control inspect is required for interrupt.",
			);
		};
		let Some(authority) = lane_control_authority(params) else {
			return lane_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Mutating lane-control calls require authority.reason, authority.source, and authority.inspectedRunId.",
			);
		};

		if authority.inspected_run_id != run_id {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_mismatch",
				"authority.inspectedRunId must match the requested runId.",
			);
		}
		if params.force.unwrap_or(false) && !authority.allow_hard_fallback {
			return lane_control_refusal_result(
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
				return lane_control_refusal_result(
					params,
					profile,
					"lane_interrupt_unavailable",
					format!("Lane interrupt failed closed: {error}"),
				);
			},
		};

		lane_control_interrupt_result(params, profile, report)
	}

	fn call_lane_control_steer_tool(
		&self,
		params: &LaneControlToolArgs,
		profile: McpCapabilityProfile,
	) -> Value {
		let Some(issue) = non_empty_string(params.issue.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"issue_required",
				"`issue` is required for lane-control steer.",
			);
		};
		let Some(run_id) = non_empty_string(params.run_id.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_missing",
				"`runId` from lane-control inspect is required for steer.",
			);
		};
		let Some(expected_turn_id) = non_empty_string(params.expected_turn_id.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"expected_turn_id_required",
				"`expectedTurnId` from lane-control inspect is required for steer.",
			);
		};
		let Some(message) = non_empty_string(params.message.as_deref()) else {
			return lane_control_refusal_result(
				params,
				profile,
				"message_required",
				"`message` is required for steer and is never echoed in MCP results.",
			);
		};
		let Some(authority) = lane_control_authority(params) else {
			return lane_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Mutating lane-control calls require authority.reason, authority.source, and authority.inspectedRunId.",
			);
		};

		if authority.inspected_run_id != run_id {
			return lane_control_refusal_result(
				params,
				profile,
				"inspect_first_precondition_mismatch",
				"authority.inspectedRunId must match the requested runId.",
			);
		}
		if authority.expected_turn_id != Some(expected_turn_id) {
			return lane_control_refusal_result(
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
				.and_then(|project_id| non_empty_string(Some(project_id))),
			issue,
			run_id,
			expected_turn_id,
			message,
			source: authority.source,
			wait_timeout: DEFAULT_STEER_RESULT_WAIT_TIMEOUT,
		}) {
			Ok(report) => report,
			Err(error) => {
				return lane_control_refusal_result(
					params,
					profile,
					"lane_steer_unavailable",
					format!("Lane steer failed closed: {error}"),
				);
			},
		};

		lane_control_steer_result(params, profile, report)
	}
}

fn lane_control_preconditions(params: &LaneControlToolArgs) -> Value {
	let authority = params.authority.as_ref();

	serde_json::json!({
		"project_id_present": non_empty_string(params.project_id.as_deref()).is_some(),
		"issue_present": non_empty_string(params.issue.as_deref()).is_some(),
		"run_id_present": non_empty_string(params.run_id.as_deref()).is_some(),
		"expected_turn_id_present": non_empty_string(params.expected_turn_id.as_deref()).is_some(),
		"message_present": non_empty_string(params.message.as_deref()).is_some(),
		"force_requested": params.force.unwrap_or(false),
		"authority_reason_present": authority
			.and_then(|value| non_empty_string(value.reason.as_deref()))
			.is_some(),
		"authority_source_present": authority
			.and_then(|value| non_empty_string(value.source.as_deref()))
			.is_some(),
		"authority_inspected_run_id_present": authority
			.and_then(|value| non_empty_string(value.inspected_run_id.as_deref()))
			.is_some(),
		"authority_expected_turn_id_present": authority
			.and_then(|value| non_empty_string(value.expected_turn_id.as_deref()))
			.is_some(),
		"authority_allow_hard_fallback": authority
			.and_then(|value| value.allow_hard_fallback)
			.unwrap_or(false)
	})
}

fn lane_control_authority(params: &LaneControlToolArgs) -> Option<LaneControlAuthority<'_>> {
	let authority = params.authority.as_ref()?;

	Some(LaneControlAuthority {
		reason: non_empty_string(authority.reason.as_deref())?,
		source: non_empty_string(authority.source.as_deref())?,
		inspected_run_id: non_empty_string(authority.inspected_run_id.as_deref())?,
		expected_turn_id: non_empty_string(authority.expected_turn_id.as_deref()),
		allow_hard_fallback: authority.allow_hard_fallback.unwrap_or(false),
	})
}

fn lane_control_mutating_preconditions(report: &Value) -> Vec<Value> {
	report
		.get("runs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.map(|run| {
			serde_json::json!({
				"projectId": run.get("projectId").cloned().unwrap_or(Value::Null),
				"issueId": run.get("issueId").cloned().unwrap_or(Value::Null),
				"issueIdentifier": run.get("issueIdentifier").cloned().unwrap_or(Value::Null),
				"runId": run.get("runId").cloned().unwrap_or(Value::Null),
				"attemptNumber": run.get("attemptNumber").cloned().unwrap_or(Value::Null),
				"currentTurnId": run.get("turnId").cloned().unwrap_or(Value::Null),
				"laneControlNextAction": run
					.get("laneControlNextAction")
					.cloned()
					.unwrap_or(Value::Null),
				"softInterruptAvailable": run
					.get("softInterruptAvailable")
					.cloned()
					.unwrap_or(Value::Null),
				"hardInterruptAvailable": run
					.get("hardInterruptAvailable")
					.cloned()
					.unwrap_or(Value::Null),
				"hardInterruptRequiresForce": run
					.get("hardInterruptRequiresForce")
					.cloned()
					.unwrap_or(Value::Bool(true)),
				"authority": {
					"inspectedRunId": run.get("runId").cloned().unwrap_or(Value::Null),
					"expectedTurnId": run.get("turnId").cloned().unwrap_or(Value::Null)
				}
			})
		})
		.collect()
}

fn lane_control_refusal_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	reason: &str,
	message: impl Into<String>,
) -> Value {
	tool_refusal_value(lane_control_result_value(
		params,
		profile,
		"refused",
		reason,
		message,
		serde_json::json!({}),
	))
}

fn lane_control_interrupt_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	report: Value,
) -> Value {
	let soft = report.get("softInterrupt").unwrap_or(&Value::Null);
	let hard = report.get("hardInterrupt").unwrap_or(&Value::Null);
	let status =
		if hard.is_object() && hard.get("status").and_then(Value::as_str) != Some("unavailable") {
			"ok"
		} else {
			match soft.get("status").and_then(Value::as_str) {
				Some("delivered") => "ok",
				Some("pending") => "queued",
				_ => "refused",
			}
		};
	let reason =
		report.get("classification").and_then(Value::as_str).unwrap_or("lane_interrupt_result");
	let result = serde_json::json!({
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issue": report.get("issue").cloned().unwrap_or(Value::Null),
		"issueId": report.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": report.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": report.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"force": report.get("force").cloned().unwrap_or(Value::Bool(false)),
		"classification": report.get("classification").cloned().unwrap_or(Value::Null),
		"softInterrupt": {
			"attempted": soft.get("attempted").cloned().unwrap_or(Value::Bool(false)),
			"available": soft.get("available").cloned().unwrap_or(Value::Bool(false)),
			"status": soft.get("status").cloned().unwrap_or(Value::Null),
			"classification": soft.get("classification").cloned().unwrap_or(Value::Null),
			"method": soft.get("method").cloned().unwrap_or(Value::Null),
			"requestId": soft.get("requestId").cloned().unwrap_or(Value::Null),
			"message": soft.get("message").cloned().unwrap_or(Value::Null),
			"errorClass": soft.get("errorClass").cloned().unwrap_or(Value::Null)
		},
		"hardInterrupt": if hard.is_object() {
			serde_json::json!({
				"attempted": hard.get("attempted").cloned().unwrap_or(Value::Bool(false)),
				"status": hard.get("status").cloned().unwrap_or(Value::Null),
				"classification": hard.get("classification").cloned().unwrap_or(Value::Null),
				"signals": hard.get("signals").cloned().unwrap_or_else(|| serde_json::json!([])),
				"message": hard.get("message").cloned().unwrap_or(Value::Null),
				"errorClass": hard.get("errorClass").cloned().unwrap_or(Value::Null)
			})
		} else {
			Value::Null
		},
		"nextAction": report.get("nextAction").cloned().unwrap_or(Value::Null)
	});
	let value = lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane interrupt completed through the existing lane-control guard path.",
		result,
	);

	if status == "refused" { tool_refusal_value(value) } else { tool_success(value) }
}

fn lane_control_steer_result(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	report: Value,
) -> Value {
	let outcome = report.get("outcome").and_then(Value::as_str).unwrap_or("unknown");
	let delivery_status = report.get("deliveryStatus").and_then(Value::as_str).unwrap_or("unknown");
	let failure_class = report.get("failureClass").and_then(Value::as_str);
	let status = if delivery_status == "queued" {
		"queued"
	} else if matches!(outcome, "rejected" | "failed" | "timed_out" | "fallback") {
		"refused"
	} else {
		"ok"
	};
	let reason = failure_class
		.or_else(|| report.get("reason").and_then(Value::as_str))
		.unwrap_or("lane_steer_result");
	let result = serde_json::json!({
		"projectId": report.get("projectId").cloned().unwrap_or(Value::Null),
		"issueId": report.get("issueId").cloned().unwrap_or(Value::Null),
		"issueIdentifier": report.get("issueIdentifier").cloned().unwrap_or(Value::Null),
		"runId": report.get("runId").cloned().unwrap_or(Value::Null),
		"attemptNumber": report.get("attemptNumber").cloned().unwrap_or(Value::Null),
		"expectedTurnId": report.get("expectedTurnId").cloned().unwrap_or(Value::Null),
		"currentTurnId": report.get("currentTurnId").cloned().unwrap_or(Value::Null),
		"responseTurnId": report.get("responseTurnId").cloned().unwrap_or(Value::Null),
		"auditRecordId": report.get("auditRecordId").cloned().unwrap_or(Value::Null),
		"requestId": report.get("requestId").cloned().unwrap_or(Value::Null),
		"outcome": report.get("outcome").cloned().unwrap_or(Value::Null),
		"reason": report.get("reason").cloned().unwrap_or(Value::Null),
		"failureClass": report.get("failureClass").cloned().unwrap_or(Value::Null),
		"deliveryStatus": report.get("deliveryStatus").cloned().unwrap_or(Value::Null),
		"messageByteCount": report.get("messageByteCount").cloned().unwrap_or(Value::Null),
		"messageLineCount": report.get("messageLineCount").cloned().unwrap_or(Value::Null)
	});
	let value = lane_control_result_value(
		params,
		profile,
		status,
		reason,
		"Lane steer returned without exposing the original steer message.",
		result,
	);

	if status == "refused" { tool_refusal_value(value) } else { tool_success(value) }
}

fn lane_control_result_value(
	params: &LaneControlToolArgs,
	profile: McpCapabilityProfile,
	status: &str,
	reason: &str,
	message: impl Into<String>,
	result: Value,
) -> Value {
	let mut value = serde_json::json!({
		"schema": "decodex.mcp.lane_control_result/1",
		"status": status,
		"reason": reason,
		"message": message.into(),
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"project_id": params.project_id.as_deref(),
		"issue": params.issue.as_deref(),
		"run_id": params.run_id.as_deref(),
		"preconditions": lane_control_preconditions(params),
		"result": result
	});

	sanitize_mcp_observability_value(&mut value);

	value
}
