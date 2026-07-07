use crate::{
	mcp::{
		McpContext, McpError,
		resources::{
			DECISION_CONTRACTS_HOST, DOCS_HOST, PROJECTS_HOST,
			types::{ResourceContent, ResourceUri},
		},
	},
	prelude::Result,
};

impl McpContext {
	pub(in crate::mcp::resources) fn read_resource(
		&self,
		uri: &str,
	) -> Result<ResourceContent, McpError> {
		let resource_uri = ResourceUri::parse(uri)?;

		match resource_uri.host.as_str() {
			DOCS_HOST => self.read_docs_resource(&resource_uri),
			DECISION_CONTRACTS_HOST => self.read_decision_contract_resource(&resource_uri),
			PROJECTS_HOST => self.read_project_resource(&resource_uri),
			_ => Err(McpError::resource_not_found()),
		}
	}
}
