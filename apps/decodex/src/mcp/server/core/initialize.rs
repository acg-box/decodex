use serde_json::Value;

use crate::mcp::{
	MCP_HTTP_ENDPOINT_PATH, MCP_PROTOCOL_VERSION, MCP_SESSION_HEADER, McpCapabilityProfile,
	SERVER_NAME, server::core::McpServer,
};

impl McpServer {
	pub(super) fn initialize(&self) -> Value {
		serde_json::json!({
			"protocolVersion": MCP_PROTOCOL_VERSION,
			"capabilities": {
				"resources": {},
				"prompts": {},
				"tools": {},
				"logging": {},
				"experimental": {
					"decodex": {
						"capabilityProfile": self.capability_profile.as_str(),
						"capabilityProfiles": McpCapabilityProfile::ALL
							.into_iter()
							.map(McpCapabilityProfile::as_str)
							.collect::<Vec<_>>(),
						"transport": self.transport.as_str(),
						"remoteControl": {
							"safeDefaultProfile": "observe",
							"httpTransport": "streamable-http",
							"httpEndpoint": MCP_HTTP_ENDPOINT_PATH,
							"sessionHeader": MCP_SESSION_HEADER,
							"sseResponses": true,
							"originValidation": true,
							"operateAdminTools": "inspect_first_guarded",
							"mutatingToolsRequireAuthority": true,
							"privateEvidencePayloadsExposed": false
						}
					}
				}
			},
			"serverInfo": {
				"name": SERVER_NAME,
				"version": env!("CARGO_PKG_VERSION")
			}
		})
	}
}
