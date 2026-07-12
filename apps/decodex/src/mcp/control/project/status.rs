use serde_json::Value;

use crate::{
	mcp::{
		McpCapabilityProfile,
		control::project::{args::ProjectControlToolArgs, results},
	},
	runtime,
};

pub(super) fn project_control_status_result(
	params: &ProjectControlToolArgs,
	profile: McpCapabilityProfile,
	project_id: &str,
) -> Value {
	let state_store = match runtime::open_runtime_store_lazy_for_origin(
		crate::lane_authority::InvocationOrigin::Mcp,
	) {
		Ok(state_store) => state_store,
		Err(error) => {
			return results::project_control_refusal_result(
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
			return results::project_control_refusal_result(
				params,
				profile,
				"project_registry_unavailable",
				format!("Project registry read failed closed: {error}"),
			);
		},
	};
	let Some(project) = projects.iter().find(|project| project.service_id() == project_id) else {
		return results::project_control_refusal_result(
			params,
			profile,
			"project_not_registered",
			"Project control requires a registered Decodex project.",
		);
	};

	results::project_control_success_result(
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
