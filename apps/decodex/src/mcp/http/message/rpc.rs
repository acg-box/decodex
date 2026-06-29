use std::str;

use serde_json::{self, Value};

use crate::mcp::MCP_PROTOCOL_VERSION;

pub(in crate::mcp) fn json_rpc_method_name(body: &str) -> Option<String> {
	serde_json::from_str::<Value>(body)
		.ok()
		.and_then(|value| value.get("method").and_then(Value::as_str).map(str::to_owned))
}

pub(in crate::mcp) fn initialize_response_succeeded(responses: &[Value]) -> bool {
	responses.iter().any(|response| {
		response.get("error").is_none()
			&& response
				.get("result")
				.and_then(|result| result.get("protocolVersion"))
				.and_then(Value::as_str)
				== Some(MCP_PROTOCOL_VERSION)
	})
}
