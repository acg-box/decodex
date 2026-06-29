use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	orchestrator::{self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, McpLaneSteerRequest},
	runtime,
};

use super::{
	DEFAULT_MCP_STATUS_LIMIT, McpCapabilityProfile, McpServer, TOOL_LANE_CONTROL,
	TOOL_PROJECT_CONTROL, invalid_tool_arguments, non_empty_string,
	resources::{mcp_public_lane_inspect_resource, sanitize_mcp_observability_value},
	tool_refusal_value, tool_success,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectControlToolArgs {
	action: String,
	project_id: Option<String>,
	authority: Option<ProjectControlAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectControlAuthorityArgs {
	reason: Option<String>,
	source: Option<String>,
	acknowledge_future_dispatch_only: Option<bool>,
}

struct LaneControlAuthority<'a> {
	reason: &'a str,
	source: &'a str,
	inspected_run_id: &'a str,
	expected_turn_id: Option<&'a str>,
	allow_hard_fallback: bool,
}

struct ProjectControlAuthority<'a> {
	reason: &'a str,
	source: &'a str,
	acknowledge_future_dispatch_only: bool,
}

impl McpServer {
	pub(super) fn call_lane_control_tool(
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

	pub(super) fn call_project_control_tool(
		&self,
		arguments: Value,
		profile: McpCapabilityProfile,
	) -> Value {
		let params = match serde_json::from_value::<ProjectControlToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_PROJECT_CONTROL,
					"`action` is required and must be one of status, pause, resume, or scan.",
				);
			},
		};

		if !matches!(params.action.as_str(), "status" | "pause" | "resume" | "scan") {
			return invalid_tool_arguments(
				TOOL_PROJECT_CONTROL,
				"`action` must be one of status, pause, resume, or scan.",
			);
		}

		let Some(project_id) =
			non_empty_string(params.project_id.as_deref()).or(self.context.project_id.as_deref())
		else {
			return project_control_refusal_result(
				&params,
				profile,
				"project_id_required",
				"`projectId` is required when the MCP gateway is not bound to one project config.",
			);
		};

		if let Some(context_project_id) = self.context.project_id.as_deref()
			&& context_project_id != project_id
		{
			return project_control_refusal_result(
				&params,
				profile,
				"project_mismatch",
				"The requested projectId does not match this MCP gateway context.",
			);
		}

		match params.action.as_str() {
			"status" => project_control_status_result(&params, profile, project_id),
			"scan" => project_control_refusal_result(
				&params,
				profile,
				"operator_control_loop_required",
				"Linear scan requests are queued by the Decodex operator control-plane loop; standalone MCP serve cannot enqueue that in-memory request.",
			),
			"pause" | "resume" => self.call_project_enablement_tool(&params, profile, project_id),
			_ => unreachable!("project-control action was validated above"),
		}
	}

	fn call_project_enablement_tool(
		&self,
		params: &ProjectControlToolArgs,
		profile: McpCapabilityProfile,
		project_id: &str,
	) -> Value {
		let Some(authority) = project_control_authority(params) else {
			return project_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Project pause/resume requires authority.reason, authority.source, and authority.acknowledgeFutureDispatchOnly=true.",
			);
		};

		if !authority.acknowledge_future_dispatch_only {
			return project_control_refusal_result(
				params,
				profile,
				"future_dispatch_ack_required",
				"Project control affects future dispatch only and does not kill active lanes.",
			);
		}

		let state_store = match runtime::open_runtime_store_lazy() {
			Ok(state_store) => state_store,
			Err(error) => {
				return project_control_refusal_result(
					params,
					profile,
					"project_control_unavailable",
					format!("Project control failed closed: {error}"),
				);
			},
		};

		if let Some(config_path) = self.context.config_path.as_deref()
			&& let Err(error) = runtime::register_project_config(&state_store, config_path, true)
		{
			return project_control_refusal_result(
				params,
				profile,
				"project_registration_unavailable",
				format!("Project registration refresh failed closed: {error}"),
			);
		}

		let enabled = params.action == "resume";

		if let Err(error) = state_store.set_project_enabled(project_id, enabled) {
			return project_control_refusal_result(
				params,
				profile,
				"project_enablement_unavailable",
				format!("Project {action} failed closed: {error}", action = params.action),
			);
		}

		project_control_success_result(
			params,
			profile,
			project_id,
			serde_json::json!({
				"enabled": enabled,
				"authority_source": authority.source,
				"authority_reason_present": !authority.reason.is_empty(),
				"future_dispatch_only": true,
				"active_lanes_killed": false,
				"next_action": if enabled {
					"Future dispatch is enabled. Active lanes were not modified."
				} else {
					"Future dispatch is paused. Inspect active lanes separately before taking lane-control action."
				}
			}),
		)
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

fn project_control_authority(
	params: &ProjectControlToolArgs,
) -> Option<ProjectControlAuthority<'_>> {
	let authority = params.authority.as_ref()?;

	Some(ProjectControlAuthority {
		reason: non_empty_string(authority.reason.as_deref())?,
		source: non_empty_string(authority.source.as_deref())?,
		acknowledge_future_dispatch_only: authority
			.acknowledge_future_dispatch_only
			.unwrap_or(false),
	})
}

fn project_control_status_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
) -> Value {
	let state_store = match runtime::open_runtime_store_lazy() {
		Ok(state_store) => state_store,
		Err(error) => {
			return project_control_refusal_result(
				params,
				profile,
				"project_control_unavailable",
				format!("Project status failed closed: {error}"),
			);
		},
	};
	let projects = match state_store.list_projects() {
		Ok(projects) => projects,
		Err(error) => {
			return project_control_refusal_result(
				params,
				profile,
				"project_registry_unavailable",
				format!("Project registry read failed closed: {error}"),
			);
		},
	};
	let Some(project) = projects.iter().find(|project| project.service_id() == project_id) else {
		return project_control_refusal_result(
			params,
			profile,
			"project_not_registered",
			"Project control requires a registered Decodex project.",
		);
	};

	project_control_success_result(
		params,
		profile,
		project_id,
		serde_json::json!({
			"enabled": project.enabled(),
			"future_dispatch_only": true,
			"active_lanes_killed": false,
			"next_action": if project.enabled() {
				"Project is enabled for future dispatch."
			} else {
				"Project is paused for future dispatch. Existing lanes remain visible."
			}
		}),
	)
}

fn project_control_success_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
	result: Value,
) -> Value {
	tool_success(project_control_result_value(
		params,
		profile,
		project_id,
		"ok",
		params.action.as_str(),
		"Project control completed through the registered project enablement guard.",
		result,
	))
}

fn project_control_refusal_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	reason: &str,
	message: impl Into<String>,
) -> Value {
	let project_id = params.project_id.as_deref().unwrap_or("");

	tool_refusal_value(project_control_result_value(
		params,
		profile,
		project_id,
		"refused",
		reason,
		message,
		serde_json::json!({}),
	))
}

fn project_control_result_value(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
	status: &str,
	reason: &str,
	message: impl Into<String>,
	result: Value,
) -> Value {
	let mut value = serde_json::json!({
		"schema": "decodex.mcp.project_control_result/1",
		"status": status,
		"reason": reason,
		"message": message.into(),
		"capability_profile": profile.as_str(),
		"action": params.action.as_str(),
		"project_id": non_empty_string(Some(project_id)),
		"future_dispatch_only": true,
		"result": result
	});

	sanitize_mcp_observability_value(&mut value);

	value
}
