use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	mcp::{
		McpCapabilityProfile, McpServer, TOOL_PROJECT_CONTROL, invalid_tool_arguments,
		non_empty_string, observability::sanitize_mcp_observability_value, tool_refusal_value,
		tool_success,
	},
	runtime,
};
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

struct ProjectControlAuthority<'a> {
	reason: &'a str,
	source: &'a str,
	acknowledge_future_dispatch_only: bool,
}

impl McpServer {
	pub(in crate::mcp) fn call_project_control_tool(
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
