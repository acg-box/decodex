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
	pub(in crate::mcp::resources) fn openwiki_resources(
		&self,
	) -> Result<Vec<McpResource>, McpError> {
		listing::openwiki_resources(&self.repo_root)
	}

	pub(super) fn read_openwiki_resource(
		&self,
		uri: &ResourceUri,
	) -> Result<ResourceContent, McpError> {
		let path = match uri.segments.as_slice() {
			[segment] if segment == "quickstart" => self.repo_root.join("openwiki/quickstart.md"),
			[section, topic]
				if files::openwiki_section_allowed(section) && files::safe_resource_stem(topic) =>
				self.repo_root.join("openwiki").join(section).join(format!("{topic}.md")),
			_ => return Err(McpError::resource_not_found()),
		};

		files::read_file_resource(&uri.raw, path, "text/markdown")
	}
}
