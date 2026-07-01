use color_eyre::eyre;
use serde::Deserialize;
use serde_json::{self, Value};

#[derive(Clone, Debug)]
pub(crate) struct WireMessage {
	pub(crate) raw: String,
	pub(crate) message: JsonRpcMessage,
}
impl WireMessage {
	pub(super) fn parse(raw: String) -> crate::prelude::Result<Self> {
		let value: Value = serde_json::from_str(&raw)?;
		let message = if value.get("method").is_some() && value.get("id").is_some() {
			JsonRpcMessage::Request(serde_json::from_value(value)?)
		} else if value.get("method").is_some() {
			JsonRpcMessage::Notification(serde_json::from_value(value)?)
		} else if value.get("error").is_some() {
			JsonRpcMessage::Error(serde_json::from_value(value)?)
		} else if value.get("result").is_some() {
			JsonRpcMessage::Response(serde_json::from_value(value)?)
		} else {
			return Err(eyre::eyre!("Received an unrecognized JSON-RPC payload: {raw}"));
		};

		Ok(Self { raw, message })
	}
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
	pub(crate) id: Value,
	pub(crate) method: String,
	#[serde(default)]
	pub(crate) params: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcNotification {
	pub(crate) method: String,
	#[serde(default)]
	pub(crate) params: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcResponse {
	pub(crate) id: Value,
	pub(crate) result: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcError {
	pub(crate) id: Value,
	pub(crate) error: JsonRpcErrorPayload,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcErrorPayload {
	pub(crate) code: i64,
	pub(crate) message: String,

	pub(crate) data: Option<Value>,
}

#[derive(Clone, Debug)]
pub(crate) enum JsonRpcMessage {
	Request(JsonRpcRequest),
	Notification(JsonRpcNotification),
	Response(JsonRpcResponse),
	Error(JsonRpcError),
}
