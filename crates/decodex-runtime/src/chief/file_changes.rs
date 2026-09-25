//! Hold connection-bound file evidence until its immutable approval record commits.
use serde_json::Value;
use std::collections::HashMap;

type Key = (String, String, String, String);

impl super::ChiefCoordinator {
	pub(super) async fn request_payload(
		&self,
		id: &super::RequestId,
		method: &str,
		params: &Value,
		owner: Option<&str>,
	) -> Result<Value, super::ChiefError> {
		let mut payload = serde_json::json!({"id":id,"method":method,"params":params,"ownerThreadId":owner,"connectionId":self.client.connection_identity()});
		if method != "item/fileChange/requestApproval" {
			return Ok(payload);
		}
		if let Some(event_id) = self.pending_requests.get(id) {
			let saved = self.store.get_chief_inbox_event(*event_id).await?;
			if let Ok(previous) = serde_json::from_str::<Value>(&saved.payload)
				&& previous["method"] == method
				&& previous["params"] == *params
				&& previous["ownerThreadId"] == payload["ownerThreadId"]
				&& previous["connectionId"] == payload["connectionId"]
			{
				if let Some(file) = previous.get("fileChange") {
					payload["fileChange"] = file.clone();
				}
				return Ok(payload);
			}
		}
		if let Some(file) = self.pending_file_changes.get(self.client.connection_identity(), params)
		{
			payload["fileChange"] = file;
		}
		Ok(payload)
	}
}

#[derive(Default)]
pub(super) struct PendingFileChanges {
	items: HashMap<Key, String>,
	bytes: usize,
}
impl PendingFileChanges {
	pub(super) fn observe(&mut self, connection: &str, method: &str, params: &Value) {
		if ["thread/reverted", "thread/closed", "thread/archived", "thread/deleted"]
			.contains(&method)
		{
			self.finish(connection, params["threadId"].as_str(), None);
			return;
		}
		if method == "turn/completed" {
			if let Some(turn) = params["turn"]["id"].as_str() {
				self.finish(connection, params["threadId"].as_str(), Some(turn));
			}
			return;
		}
		if !["item/started", "item/completed"].contains(&method)
			|| params["item"]["type"] != "fileChange"
		{
			return;
		}
		let Some(key) = key(connection, params, &params["item"]["id"]) else { return };
		self.remove(&key);
		if method == "item/completed" {
			return;
		}
		let item = params["item"].to_string();
		let bytes = cost(&key, &item);
		if item.len() > decodex_core::MAX_NATIVE_MESSAGE_BYTES
			|| self.items.len() >= 32
			|| self.bytes + bytes > 32 * 1024 * 1024
		{
			return;
		}
		self.bytes += bytes;
		self.items.insert(key, item);
	}

	pub(super) fn get(&self, connection: &str, params: &Value) -> Option<Value> {
		serde_json::from_str(self.items.get(&key(connection, params, &params["itemId"])?)?).ok()
	}

	pub(super) fn committed(&mut self, connection: &str, params: &Value) {
		if let Some(key) = key(connection, params, &params["itemId"]) {
			self.remove(&key);
		}
	}

	fn remove(&mut self, key: &Key) {
		if let Some(item) = self.items.remove(key) {
			self.bytes -= cost(key, &item);
		}
	}

	fn finish(&mut self, connection: &str, thread: Option<&str>, turn: Option<&str>) {
		let Some(thread) = thread else { return };
		self.items.retain(|key, value| {
			if key.0 == connection && key.1 == thread && turn.is_none_or(|turn| key.2 == turn) {
				self.bytes -= cost(key, value);
				false
			} else {
				true
			}
		});
	}
}
fn key(connection: &str, params: &Value, item: &Value) -> Option<Key> {
	Some((
		connection.into(),
		params["threadId"].as_str().filter(|s| !s.is_empty())?.into(),
		params["turnId"].as_str().filter(|s| !s.is_empty())?.into(),
		item.as_str().filter(|s| !s.is_empty())?.into(),
	))
}
fn cost(key: &Key, item: &str) -> usize {
	key.0.len() + key.1.len() + key.2.len() + key.3.len() + item.len()
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[test]
	fn evidence_is_bound_to_connection_and_released_after_commit_or_lifecycle_end() {
		let mut state = PendingFileChanges::default();
		let started = json!({"threadId":"child","turnId":"turn","item":{"id":"patch","type":"fileChange","changes":[]}});
		let request = json!({"threadId":"child","turnId":"turn","itemId":"patch"});
		for (method, params) in [
			("item/completed", started.clone()),
			("turn/completed", json!({"threadId":"child","turn":{"id":"turn"}})),
			("thread/reverted", json!({"threadId":"child"})),
			("thread/closed", json!({"threadId":"child"})),
		] {
			state.observe("connection", "item/started", &started);
			assert!(state.get("foreign", &request).is_none());
			for field in ["threadId", "turnId", "itemId"] {
				let mut wrong = request.clone();
				wrong[field] = json!("foreign");
				assert!(state.get("connection", &wrong).is_none());
			}
			assert_eq!(state.get("connection", &request), Some(started["item"].clone()));
			assert_eq!(
				state.get("connection", &request),
				Some(started["item"].clone()),
				"reads do not discard uncommitted evidence"
			);
			state.observe("connection", method, &params);
			assert_eq!(state.bytes, 0);
		}
		state.observe("connection", "item/started", &started);
		state.committed("connection", &request);
		assert_eq!(state.bytes, 0);
		assert!(state.items.is_empty());
	}
}
