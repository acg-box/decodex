use serde_json::Value;

use crate::mcp::{
	self, McpError,
	server::{
		core::McpServer,
		protocol::{self, JsonRpcRequest},
	},
};

impl McpServer {
	pub(super) fn handle_request(
		&self,
		request: JsonRpcRequest,
		emit_progress: bool,
	) -> Vec<Value> {
		let Some(id) = request.id else {
			return Vec::new();
		};

		if request.jsonrpc.as_deref() != Some("2.0") {
			return vec![protocol::json_rpc_error(id, -32_600, "Invalid Request")];
		}

		let Some(method) = request.method else {
			return vec![protocol::json_rpc_error(id, -32_600, "Invalid Request")];
		};
		let progress_token = emit_progress
			.then(|| protocol::progress_token_from_params(request.params.as_ref()))
			.flatten();
		let result = match method.as_str() {
			"initialize" => Ok(self.initialize()),
			"ping" => Ok(serde_json::json!({})),
			"logging/setLevel" => Ok(serde_json::json!({})),
			"resources/list" => self.list_resources(),
			"resources/read" => self.read_resource(request.params),
			"resources/templates/list" => Ok(self.list_resource_templates()),
			"prompts/list" => Ok(self.list_prompts()),
			"prompts/get" => self.get_prompt(request.params),
			"tools/list" => Ok(self.list_tools()),
			"tools/call" => self.call_tool(request.params),
			_ => Err(McpError::method_not_found()),
		};
		let mut responses = Vec::new();

		if method == "tools/call"
			&& result.as_ref().is_ok_and(mcp::tool_call_result_allows_progress)
			&& let Some(token) = progress_token
		{
			responses.push(protocol::progress_notification(
				token,
				1,
				Some(2),
				"Decodex MCP tool request accepted.",
			));
		}

		responses.push(match result {
			Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
			Err(error) => protocol::json_rpc_error(id, error.code, &error.message),
		});

		responses
	}
}
