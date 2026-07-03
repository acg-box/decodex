use color_eyre::eyre::Report;

use crate::{
	agent::{
		app_server::{
			DynamicToolHandler, DynamicToolSpec,
			dynamic_tools::failure::AppServerDynamicToolFailure,
		},
		tracker_tool_bridge,
	},
	prelude::Result,
};

pub(in crate::agent::app_server) fn validated_dynamic_tool_specs(
	handler: &dyn DynamicToolHandler,
) -> Result<Vec<DynamicToolSpec>> {
	let tool_specs = handler.tool_specs();

	for spec in &tool_specs {
		if !tracker_tool_bridge::dynamic_tool_identifier_is_valid(&spec.name) {
			return Err(Report::new(AppServerDynamicToolFailure::protocol(
				Some(spec.name.clone()),
				format!(
					"Dynamic tool name `{}` does not match the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
					spec.name
				),
			)));
		}

		if let Some(namespace) = spec.namespace.as_deref()
			&& !tracker_tool_bridge::dynamic_tool_identifier_is_valid(namespace)
		{
			return Err(Report::new(AppServerDynamicToolFailure::protocol(
				Some(format!("{namespace}.{}", spec.name)),
				format!(
					"Dynamic tool namespace `{namespace}` does not match the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`."
				),
			)));
		}
	}

	Ok(tool_specs)
}
