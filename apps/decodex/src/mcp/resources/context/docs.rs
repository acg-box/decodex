mod files;
mod listing;

use crate::{
	mcp::{
		McpContext, McpError,
		resources::types::{McpResource, ResourceContent, ResourceUri},
	},
	prelude::Result,
};

impl McpContext {
	pub(in crate::mcp::resources) fn docs_resources(&self) -> Result<Vec<McpResource>, McpError> {
		listing::docs_resources(&self.repo_root)
	}

	pub(super) fn read_docs_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "index" => self.repo_root.join("docs/index.md"),
			[segment] if segment == "policy" => self.repo_root.join("docs/policy.md"),
			[lane, topic] if files::docs_lane_allowed(lane) && files::safe_resource_stem(topic) => {
				self.repo_root.join("docs").join(lane).join(format!("{topic}.md"))
			},
			_ => return Err(McpError::resource_not_found()),
		};

		files::read_file_resource(&uri.raw, path, "text/markdown")
	}
}
