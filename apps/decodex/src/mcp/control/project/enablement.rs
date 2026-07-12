use serde_json::Value;

use crate::{
	mcp::{
		McpCapabilityProfile, McpServer,
		control::project::{args, args::ProjectControlToolArgs, results},
	},
	runtime,
};

impl McpServer {
	pub(super) fn call_project_enablement_tool(
		&self,
		params: &ProjectControlToolArgs,
		profile: McpCapabilityProfile,
		project_id: &str,
	) -> Value {
		let Some(authority) = args::project_control_authority(params) else {
			return results::project_control_refusal_result(
				params,
				profile,
				"authority_required",
				"Project pause/resume requires authority.reason, authority.source, and authority.acknowledgeFutureDispatchOnly=true.",
			);
		};

		if !authority.acknowledge_future_dispatch_only {
			return results::project_control_refusal_result(
				params,
				profile,
				"future_dispatch_ack_required",
				"Project control affects future dispatch only and does not kill active lanes.",
			);
		}

		let state_store = match runtime::open_runtime_store_lazy_for_origin(
			crate::lane_authority::InvocationOrigin::Mcp,
		) {
			Ok(state_store) => state_store,
			Err(error) => {
				return results::project_control_refusal_result(
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
			return results::project_control_refusal_result(
				params,
				profile,
				"project_registration_unavailable",
				format!("Project registration refresh failed closed: {error}"),
			);
		}

		let enabled = params.action == "resume";

		if let Err(error) = state_store.set_project_enabled(project_id, enabled) {
			return results::project_control_refusal_result(
				params,
				profile,
				"project_enablement_unavailable",
				format!("Project {action} failed closed: {error}", action = params.action),
			);
		}

		results::project_control_success_result(
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
