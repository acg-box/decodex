//! Native item pages also work for archived threads, unlike timeline/list.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{AppServerClient, ClientError};

impl AppServerClient {
	/// Read stored items and their turn headers without loading an execution session.
	pub async fn thread_saved_history_page(
		&self,
		thread: &str,
		cursor: Option<&str>,
		limit: u32,
	) -> Result<Value, ClientError> {
		if thread.is_empty()
			|| thread.len() > 512
			|| !(1..=30).contains(&limit)
			|| cursor.is_some_and(|value| value.is_empty() || value.len() > 4096)
		{
			return Err(ClientError::InvalidFrame);
		}
		let metadata = self.thread_read(json!({"threadId":thread})).await?;
		if metadata["thread"]["id"] != thread {
			return Err(ClientError::InvalidFrame);
		}
		let page = self
			.request(
				"thread/items/list",
				json!({
					"threadId":thread,"cursor":cursor,"limit":limit,"sortDirection":"desc"
				}),
			)
			.await?;
		let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
		if data.len() > limit as usize {
			return Err(ClientError::CapacityExceeded);
		}
		match page.get("nextCursor") {
			Some(Value::Null) => {},
			Some(Value::String(next))
				if !next.is_empty()
					&& next.len() <= 4096
					&& Some(next.as_str()) != cursor
					&& !data.is_empty() => {},
			_ => return Err(ClientError::InvalidFrame),
		}
		let mut needed = BTreeSet::new();
		let mut identities = BTreeSet::new();
		for item in data {
			let turn = item["turnId"]
				.as_str()
				.filter(|s| !s.is_empty() && s.len() <= 512)
				.ok_or(ClientError::InvalidFrame)?;
			let id = item["item"]["id"]
				.as_str()
				.filter(|s| !s.is_empty() && s.len() <= 512)
				.ok_or(ClientError::InvalidFrame)?;
			if !identities.insert((turn, id)) {
				return Err(ClientError::InvalidFrame);
			}
			needed.insert(turn.to_owned());
		}
		let mut headers = BTreeMap::new();
		let mut after: Option<String> = None;
		let mut seen = BTreeSet::new();
		for _ in 0..128 {
			if needed.is_empty() {
				break;
			}
			let turns = self
				.request(
					"thread/turns/list",
					json!({
						"threadId":thread,"cursor":after,"limit":100,
						"sortDirection":"desc","itemsView":"notLoaded"
					}),
				)
				.await?;
			for turn in turns["data"].as_array().ok_or(ClientError::InvalidFrame)? {
				let id = turn["id"].as_str().ok_or(ClientError::InvalidFrame)?;
				if needed.remove(id) {
					headers.insert(id.to_owned(), turn.clone());
				}
			}
			match turns.get("nextCursor") {
				Some(Value::Null) => break,
				Some(Value::String(next))
					if !next.is_empty() && next.len() <= 4096 && seen.insert(next.clone()) =>
					after = Some(next.clone()),
				_ => return Err(ClientError::InvalidFrame),
			}
		}
		if !needed.is_empty() {
			return Err(ClientError::InvalidFrame);
		}
		Ok(json!({"threadId":thread,"data":data.iter().rev().collect::<Vec<_>>(),
			"turns":headers,"nextCursor":page["nextCursor"]}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	#[tokio::test]
	async fn archived_items_keep_the_native_cursor_and_do_not_resume_the_thread() {
		let (local, remote) = tokio::io::duplex(65536);
		let (read, write) = tokio::io::split(local);
		let (client, _) = AppServerClient::from_io(read, write);
		let server = tokio::spawn(async move {
			let (read, mut write) = tokio::io::split(remote);
			let mut lines = BufReader::new(read).lines();
			for (method, result) in [
				("thread/read", json!({"thread":{"id":"thread"}})),
				(
					"thread/items/list",
					json!({"data":[
					{"turnId":"t","item":{"id":"answer","type":"agentMessage","text":"Done"}},
					{"turnId":"t","item":{"id":"process","type":"reasoning","summary":["Checking"]}}
				],"nextCursor":"native-older"}),
				),
				(
					"thread/turns/list",
					json!({"data":[{"id":"t","status":"completed","durationMs":1250}],"nextCursor":null}),
				),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				if method == "thread/items/list" {
					assert_eq!(request["params"]["cursor"], "native-newer");
				}
				write
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let page =
			client.thread_saved_history_page("thread", Some("native-newer"), 2).await.unwrap();
		assert_eq!(page["data"][0]["item"]["id"], "process");
		assert_eq!(page["turns"]["t"]["durationMs"], 1250);
		assert_eq!(page["nextCursor"], "native-older");
		server.await.unwrap();
	}
}
