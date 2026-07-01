use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Value;

use super::{ThreadGoal, TurnError, TurnStatusPayload, string_like_json_value};

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadStatusChangedNotification {
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) status: ThreadStatus,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadStatus {
	#[serde(rename = "type")]
	pub(in crate::agent::app_server) kind: String,
	#[serde(default, rename = "activeFlags")]
	pub(in crate::agent::app_server) active_flags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct AgentMessageDeltaNotification {
	pub(in crate::agent::app_server) delta: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ItemCompletedNotification {
	pub(in crate::agent::app_server) item: CompletedItem,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct CompletedItem {
	#[serde(rename = "type")]
	pub(in crate::agent::app_server) kind: String,
	pub(in crate::agent::app_server) text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct TurnCompletedNotification {
	#[allow(dead_code)]
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: Option<String>,
	pub(in crate::agent::app_server) turn: TurnStatusPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalUpdatedNotification {
	pub(in crate::agent::app_server) goal: ThreadGoal,
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) turn_id: Option<String>,
}

#[derive(Debug)]
pub(in crate::agent::app_server) struct ErrorNotification {
	pub(in crate::agent::app_server) error: TurnError,
	pub(in crate::agent::app_server) will_retry: Option<bool>,
	pub(in crate::agent::app_server) thread_id: Option<String>,
	pub(in crate::agent::app_server) turn_id: Option<String>,
}
impl<'de> Deserialize<'de> for ErrorNotification {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let value = Value::deserialize(deserializer)?;
		let entries = value
			.as_object()
			.ok_or_else(|| Error::custom("expected app-server error notification object"))?;
		let error_value = entries
			.get("error")
			.ok_or_else(|| Error::custom("expected app-server error notification error"))?;
		let error = TurnError::deserialize(error_value.clone()).map_err(Error::custom)?;
		let will_retry =
			entries.get("willRetry").or_else(|| entries.get("will_retry")).and_then(Value::as_bool);
		let thread_id = entries
			.get("threadId")
			.or_else(|| entries.get("thread_id"))
			.and_then(string_like_json_value);
		let turn_id = entries
			.get("turnId")
			.or_else(|| entries.get("turn_id"))
			.and_then(string_like_json_value);

		Ok(Self { error, will_retry, thread_id, turn_id })
	}
}
