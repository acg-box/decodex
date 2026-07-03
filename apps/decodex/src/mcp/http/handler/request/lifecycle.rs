use serde_json::Value;

use crate::mcp::{
	self, MCP_SESSION_HEADER,
	http::{
		handler::request::McpHttpHandler,
		message::{McpHttpRequest, McpHttpResponse},
	},
};

impl McpHttpHandler {
	pub(super) fn handle_options(&self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(method) = request.header("Access-Control-Request-Method") else {
			return McpHttpResponse::empty("204 No Content");
		};

		if matches!(method.to_ascii_uppercase().as_str(), "POST" | "DELETE") {
			McpHttpResponse::empty("204 No Content")
		} else {
			McpHttpResponse::empty("405 Method Not Allowed")
		}
	}

	pub(super) fn handle_delete(&mut self, request: &McpHttpRequest) -> McpHttpResponse {
		let Some(session_id) = request.header(MCP_SESSION_HEADER) else {
			return McpHttpResponse::json_error(
				"428 Precondition Required",
				mcp::json_rpc_error(Value::Null, -32_000, "Missing MCP session"),
			);
		};

		if !self.sessions.remove(session_id) {
			return McpHttpResponse::json_error(
				"404 Not Found",
				mcp::json_rpc_error(Value::Null, -32_001, "Unknown MCP session"),
			);
		}

		McpHttpResponse::empty("202 Accepted")
	}
}
