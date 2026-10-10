//! Native read receipts. Reading does not acknowledge a conversation.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time;

/// The first unread position in a durable native conversation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NativeUnreadPosition {
	/// An explicit unread mark, including an empty conversation.
	ThreadStart,
	/// The earliest unseen terminal result.
	Turn {
		/// Exact native turn identity.
		#[serde(rename = "turnId")]
		turn_id: String,
	},
}
/// A native receipt. Its revision has meaning only within this thread and source.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeReadState {
	/// Required nullable position; missing metadata must not mean read.
	#[serde(deserialize_with = "Option::deserialize")]
	pub first_unread: Option<NativeUnreadPosition>,
	/// Native compare-and-set token.
	pub revision: String,
}
fn valid_id(value: &str) -> bool {
	!value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}
/// Admit only explicit, revision-bound native receipt writes.
pub fn is_thread_read_state_update(value: &Value) -> bool {
	value.as_object().is_some_and(|v| v.len() == 3)
		&& value["threadId"].as_str().is_some_and(valid_id)
		&& value["expectedRevision"].as_str().is_some_and(valid_id)
		&& matches!(value["operation"]["type"].as_str(), Some("read" | "unread"))
		&& value["operation"].as_object().is_some_and(|v| v.len() == 1)
}
impl AppServerClient {
	/// Read metadata without loading turns, starting a turn, or marking anything read.
	pub async fn thread_read_state(
		&self,
		thread: &str,
	) -> Result<Option<NativeReadState>, ClientError> {
		if !valid_id(thread) {
			return Err(ClientError::InvalidFrame);
		}
		let result = time::timeout(
			Duration::from_secs(20),
			self.request(
				"thread/read",
				serde_json::json!({"threadId":thread,"includeTurns":false}),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if result["thread"]["id"] != thread {
			return Err(ClientError::InvalidFrame);
		}
		let Some(state) = result.get("readState").filter(|v| !v.is_null()) else {
			return Ok(None);
		};
		parse_state(state.clone()).map(Some)
	}

	/// Submit one revision-bound change. Conflicts and transport failures are never replayed.
	pub async fn update_thread_read_state(
		&self,
		thread: &str,
		revision: &str,
		read: bool,
		guard: HistoryGuard,
	) -> Result<NativeReadState, ClientError> {
		let params = serde_json::json!({"threadId":thread,"expectedRevision":revision,"operation":{"type":if read {"read"} else {"unread"}}});
		if !is_thread_read_state_update(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let result = time::timeout(
			Duration::from_secs(20),
			self.request_with_history("thread/readState/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		parse_state(result["readState"].clone())
	}
}
fn parse_state(value: Value) -> Result<NativeReadState, ClientError> {
	let state: NativeReadState =
		serde_json::from_value(value).map_err(|_| ClientError::InvalidFrame)?;
	if !valid_id(&state.revision)
		|| matches!(&state.first_unread,Some(NativeUnreadPosition::Turn{turn_id}) if !valid_id(turn_id))
	{
		return Err(ClientError::InvalidFrame);
	}
	Ok(state)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn missing_or_unknown_read_positions_never_mean_read() {
		for value in [
			serde_json::json!({"revision":"a"}),
			serde_json::json!({"revision":"a","firstUnread":{"type":"future"}}),
			serde_json::json!({"revision":"","firstUnread":null}),
		] {
			assert!(parse_state(value).is_err());
		}
		assert!(
			parse_state(serde_json::json!({"revision":"a","firstUnread":null}))
				.unwrap()
				.first_unread
				.is_none()
		);
	}
}
