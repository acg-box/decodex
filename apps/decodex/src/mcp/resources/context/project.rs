mod autonomy;
mod runtime;

use crate::{
	mcp::{
		McpContext, McpError,
		resources::types::{ResourceContent, ResourceUri},
	},
	prelude::Result,
};

impl McpContext {
	pub(super) fn read_project_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let [project_id, resource_kind, rest @ ..] = uri.segments.as_slice() else {
			return Err(McpError::resource_not_found());
		};

		if Some(project_id.as_str()) != self.project_id.as_deref() {
			return Err(McpError::resource_not_found());
		}
		if resource_kind == "autonomy" {
			let value = self.read_autonomy_project_resource(project_id, rest)?;

			return ResourceContent::mcp_observability_json(&uri.raw, value);
		}

		let Some(config_path) = self.config_path.as_deref() else {
			return Err(McpError::resource_not_found());
		};
		let value = runtime::read_project_runtime_resource(config_path, resource_kind, rest)?;

		ResourceContent::mcp_observability_json(&uri.raw, value)
	}
}
