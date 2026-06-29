use serde::Deserialize;
use serde_json::{self, Value};

#[derive(Deserialize)]
pub(super) struct JsonRpcRequest {
	pub(super) jsonrpc: Option<String>,
	pub(super) id: Option<Value>,
	pub(super) method: Option<String>,
	pub(super) params: Option<Value>,
}

#[derive(Deserialize)]
pub(super) struct CallToolParams {
	pub(super) name: String,
	pub(super) arguments: Option<Value>,
}

pub(in crate::mcp) fn json_rpc_error(id: Value, code: i64, message: &str) -> Value {
	serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"error": {
			"code": code,
			"message": message
		}
	})
}

pub(super) fn progress_token_from_params(params: Option<&Value>) -> Option<Value> {
	let token = params?.get("_meta")?.get("progressToken")?;

	if token.is_string() || token.is_i64() || token.is_u64() {
		return Some(token.clone());
	}

	None
}

pub(super) fn progress_notification(
	progress_token: Value,
	progress: u64,
	total: Option<u64>,
	message: &str,
) -> Value {
	let mut params = serde_json::json!({
		"progressToken": progress_token,
		"progress": progress,
		"message": message
	});

	if let Some(total) = total {
		params["total"] = serde_json::json!(total);
	}

	serde_json::json!({
		"jsonrpc": "2.0",
		"method": "notifications/progress",
		"params": params
	})
}
