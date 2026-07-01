use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Value;

use super::string_like_json_value;

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct TurnStatusPayload {
	pub(in crate::agent::app_server) id: String,
	pub(in crate::agent::app_server) status: String,
	pub(in crate::agent::app_server) error: Option<TurnError>,
}

#[derive(Debug)]
pub(in crate::agent::app_server) struct TurnError {
	pub(in crate::agent::app_server) message: String,
	pub(in crate::agent::app_server) codex_error_info: Option<String>,
	#[allow(dead_code)]
	pub(in crate::agent::app_server) additional_details: Option<Value>,
}
impl<'de> Deserialize<'de> for TurnError {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = Value::deserialize(deserializer)?;
		let entries = value
			.as_object()
			.ok_or_else(|| Error::custom("expected app-server turn error object"))?;
		let message = entries
			.get("message")
			.and_then(string_like_json_value)
			.ok_or_else(|| Error::custom("expected app-server turn error message"))?;
		let codex_error_info = entries
			.get("codexErrorInfo")
			.or_else(|| entries.get("codex_error_info"))
			.and_then(string_like_json_value);
		let additional_details =
			entries.get("additionalDetails").or_else(|| entries.get("additional_details")).cloned();

		Ok(Self { message, codex_error_info, additional_details })
	}
}
