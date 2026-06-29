mod initialize;
mod request;
mod tools;

use serde_json::Value;

use crate::mcp::{
	McpCapabilityProfile, McpContext, McpTransport,
	server::protocol::{self, JsonRpcRequest},
};

pub(in crate::mcp) struct McpServer {
	pub(in crate::mcp) context: McpContext,
	pub(in crate::mcp) capability_profile: McpCapabilityProfile,
	pub(in crate::mcp) transport: McpTransport,
}
impl McpServer {
	pub(in crate::mcp) fn handle_line(&self, line: &str, emit_progress: bool) -> Vec<Value> {
		let parsed = serde_json::from_str::<Value>(line);
		let value = match parsed {
			Ok(value) => value,
			Err(_) => return vec![protocol::json_rpc_error(Value::Null, -32_700, "Parse error")],
		};
		let request = match serde_json::from_value::<JsonRpcRequest>(value) {
			Ok(request) => request,
			Err(_) => {
				return vec![protocol::json_rpc_error(Value::Null, -32_600, "Invalid Request")];
			},
		};

		self.handle_request(request, emit_progress)
	}
}
