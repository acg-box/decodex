//! Native mixed history pages. The cursor moves backward; each page is chronological.

use super::{AppServerClient, ClientError};
use serde_json::{Value, json};

impl AppServerClient {
	/// Read a bounded native timeline without resuming a thread or executing a turn.
	/// Keep the cursor bound to this exact thread. A method-not-found response can
	/// describe a legacy thread, so callers must not cache it as process-wide absence.
	pub async fn thread_timeline_page(
		&self,
		thread: &str,
		cursor: Option<&str>,
		limit: u32,
	) -> Result<Value, ClientError> {
		if !valid_id(thread)
			|| !(1..=100).contains(&limit)
			|| cursor.is_some_and(|cursor| cursor.is_empty() || cursor.len() > 4096)
		{
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(std::time::Duration::from_secs(20), async {
			let metadata = self.thread_read(json!({"threadId":thread})).await?;
			if metadata.pointer("/thread/id").and_then(Value::as_str) != Some(thread) {
				return Err(ClientError::InvalidFrame);
			}
			let page = self
				.request(
					"thread/timeline/list",
					json!({"threadId":thread,"cursor":cursor,"limit":limit}),
				)
				.await?;
			validate_page(&page, cursor, limit)?;
			Ok(page)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}
}

fn valid_id(id: &str) -> bool {
	!id.is_empty() && id.len() <= 512
}

fn identity(value: &Value) -> Result<&str, ClientError> {
	value.as_str().filter(|id| valid_id(id)).ok_or(ClientError::InvalidFrame)
}

fn validate_page(page: &Value, cursor: Option<&str>, limit: u32) -> Result<(), ClientError> {
	let rows = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
	if rows.len() > limit as usize {
		return Err(ClientError::CapacityExceeded);
	}
	match page.get("nextCursor") {
		Some(Value::Null) => {},
		Some(Value::String(next))
			if !rows.is_empty()
				&& !next.is_empty()
				&& next.len() <= 4096
				&& Some(next.as_str()) != cursor => {},
		_ => return Err(ClientError::InvalidFrame),
	}
	match page.get("activeRealtimeSessionAtPageStart") {
		Some(Value::Null) => {},
		Some(value) => {
			identity(value)?;
		},
		None => return Err(ClientError::InvalidFrame),
	}
	let mut previous = None;
	for row in rows {
		let key = entry_key(row)?;
		if previous.is_some_and(|previous| previous >= key) {
			return Err(ClientError::InvalidFrame);
		}
		previous = Some(key);
	}
	Ok(())
}

fn entry_key(row: &Value) -> Result<(u64, u8, &str), ClientError> {
	let position = row["position"]
		.as_u64()
		.filter(|n| *n <= i64::MAX as u64)
		.ok_or(ClientError::InvalidFrame)?;
	let (kind, id) = match row["type"].as_str() {
		Some("turnStarted") => (0, identity(&row["turnId"])?),
		Some("item") => {
			identity(&row["turnId"])?;
			identity(&row["item"]["type"])?;
			(1, identity(&row["item"]["id"])?)
		},
		Some("realtime") => {
			identity(&row["item"]["realtimeSessionId"])?;
			identity(&row["item"]["type"])?;
			(2, identity(&row["item"]["id"])?)
		},
		Some("turnCompleted") => (3, identity(&row["turnId"])?),
		_ => return Err(ClientError::InvalidFrame),
	};
	Ok((position, kind, id))
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn legacy_refusal_does_not_disable_another_threads_timeline() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for (method, thread, mut response) in [
				("thread/read", "legacy", json!({"result":{"thread":{"id":"legacy"}}})),
				(
					"thread/timeline/list",
					"legacy",
					json!({"error":{"code":-32601,"message":"unsupported"}}),
				),
				("thread/read", "paged", json!({"result":{"thread":{"id":"paged"}}})),
				(
					"thread/timeline/list",
					"paged",
					json!({"result":{"data":[],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}}),
				),
				("thread/read", "wrong", json!({"result":{"thread":{"id":"unrelated"}}})),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				assert_eq!(request["params"]["threadId"], thread);
				if method == "thread/timeline/list" {
					assert_eq!(
						request["params"],
						json!({"threadId":thread,"limit":10,"cursor":null})
					);
				}
				response["id"] = request["id"].clone();
				writer.write_all(format!("{response}\n").as_bytes()).await.unwrap();
			}
		});
		assert!(matches!(client.thread_timeline_page("legacy", None, 10).await,
			Err(ClientError::Remote(error)) if error.code == -32601));
		assert!(client.thread_timeline_page("paged", None, 10).await.is_ok());
		assert!(matches!(
			client.thread_timeline_page("wrong", None, 10).await,
			Err(ClientError::InvalidFrame)
		));
		server.await.unwrap();
	}

	#[test]
	fn equal_positions_keep_native_kind_order_and_opening_voice_state() {
		let page = json!({"data":[
			{"type":"turnStarted","position":5,"turnId":"turn"},
			{"type":"item","position":5,"turnId":"turn","item":{"type":"userMessage","id":"message"}},
			{"type":"realtime","position":5,"item":{"type":"transcriptSegment","id":"speech","realtimeSessionId":"voice"}},
			{"type":"turnCompleted","position":5,"turnId":"turn"}
		],"nextCursor":"older","activeRealtimeSessionAtPageStart":"voice"});
		assert!(validate_page(&page, None, 4).is_ok());
		let mut reversed = page.clone();
		reversed["data"].as_array_mut().unwrap().reverse();
		assert!(validate_page(&reversed, None, 4).is_err());
		let mut duplicate = page.clone();
		duplicate["data"][1] = duplicate["data"][0].clone();
		assert!(validate_page(&duplicate, None, 4).is_err());
		assert!(validate_page(&page, Some("older"), 4).is_err());
	}

	#[test]
	fn incomplete_pages_are_not_empty_history() {
		for page in [
			json!({}),
			json!({"data":[],"nextCursor":null}),
			json!({"data":[],"nextCursor":"older","activeRealtimeSessionAtPageStart":null}),
			json!({"data":[],"nextCursor":null,"activeRealtimeSessionAtPageStart":""}),
		] {
			assert!(validate_page(&page, None, 10).is_err());
		}
		assert!(
			validate_page(
				&json!({"data":[],"nextCursor":null,"activeRealtimeSessionAtPageStart":null}),
				None,
				10
			)
			.is_ok()
		);
	}
}
