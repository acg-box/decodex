//! Read around a known native item without traversing the complete conversation.
use serde_json::Value;

use super::{AppServerClient, ClientError};

impl AppServerClient {
	/// Read a known item and its neighbors, or continue one native item page.
	pub async fn thread_item_context(
		&self,
		thread: &str,
		turn: &str,
		item: &str,
		cursor: Option<&str>,
		before: bool,
	) -> Result<Value, ClientError> {
		if [thread, turn, item]
			.into_iter()
			.any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
			|| cursor.is_some_and(|c| c.is_empty() || c.len() > 4096)
		{
			return Err(ClientError::InvalidFrame);
		}
		if let Some(cursor) = cursor {
			let page =
				self.item_context_page(thread, turn, Value::String(cursor.into()), before).await?;
			let (previous, next) = if before {
				(&page["nextCursor"], &page["backwardsCursor"])
			} else {
				(&page["backwardsCursor"], &page["nextCursor"])
			};
			return Ok(
				serde_json::json!({"data":page["data"],"previousCursor":previous,"nextCursor":next}),
			);
		}
		let anchor =
			self.thread_read_item(thread, turn, item).await?.ok_or(ClientError::InvalidFrame)?;
		let start = serde_json::json!({"type":"item","itemId":item});
		let earlier = self.item_context_page(thread, turn, start.clone(), true).await?;
		let later = self.item_context_page(thread, turn, start, false).await?;
		let mut data = earlier["data"].as_array().ok_or(ClientError::InvalidFrame)?.clone();
		data.push(serde_json::json!({"turnId":turn,"item":anchor}));
		data.extend(later["data"].as_array().ok_or(ClientError::InvalidFrame)?.iter().cloned());
		let mut ids = std::collections::HashSet::new();
		if data.iter().any(|entry| !ids.insert(entry["item"]["id"].as_str())) {
			return Err(ClientError::InvalidFrame);
		}
		Ok(
			serde_json::json!({"data":data,"previousCursor":earlier["nextCursor"],"nextCursor":later["nextCursor"]}),
		)
	}

	async fn item_context_page(
		&self,
		thread: &str,
		turn: &str,
		cursor: Value,
		before: bool,
	) -> Result<Value, ClientError> {
		let mut page = self.request("thread/items/list", serde_json::json!({
			"threadId":thread,"turnId":turn,"cursor":cursor,"limit":3,"sortDirection":if before {"desc"} else {"asc"}
		})).await?;
		for key in ["nextCursor", "backwardsCursor"] {
			match page.get(key) {
				Some(Value::Null) => {},
				Some(Value::String(next))
					if !next.is_empty()
						&& next.len() <= 4096
						&& Some(next.as_str()) != cursor.as_str() => {},
				_ => return Err(ClientError::InvalidFrame),
			}
		}
		let data = page["data"].as_array_mut().ok_or(ClientError::InvalidFrame)?;
		let mut ids = std::collections::HashSet::new();
		if data.len() > 3
			|| data.iter().any(|entry| {
				entry["turnId"] != turn
					|| entry["item"]["id"].as_str().is_none_or(|id| {
						id.is_empty() || id.len() > 512 || !ids.insert(id.to_owned())
					})
			}) {
			return Err(ClientError::InvalidFrame);
		}
		if before {
			data.reverse();
		}
		Ok(page)
	}
}

#[cfg(test)]
mod tests {
	use super::{AppServerClient, Value};
	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	fn item(turn: &str, id: &str) -> Value {
		serde_json::json!({"turnId":turn,"item":{"id":id,"type":"agentMessage","text":id}})
	}

	#[tokio::test]
	async fn known_item_reads_exclusive_neighbors_and_keeps_native_cursors() {
		let (local, remote) = tokio::io::duplex(8192);
		let (read, write) = tokio::io::split(local);
		let (client, _) = AppServerClient::from_io(read, write);
		let server = tokio::spawn(async move {
			let (read, mut write) = tokio::io::split(remote);
			let mut lines = BufReader::new(read).lines();
			for serial in 0..4 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				let response = if serial == 0 {
					assert_eq!(request["method"], "thread/items/read");
					serde_json::json!({"data":[item("turn","anchor")]})
				} else {
					assert_eq!(request["method"], "thread/items/list");
					assert_eq!(request["params"]["turnId"], "turn");
					if serial < 3 {
						assert_eq!(
							request["params"]["cursor"],
							serde_json::json!({"type":"item","itemId":"anchor"})
						);
					} else {
						assert_eq!(request["params"]["cursor"], "later-page");
					}
					assert_eq!(
						request["params"]["sortDirection"],
						if serial == 1 { "desc" } else { "asc" }
					);
					serde_json::json!({"data":if serial==1 {vec![item("turn","before-2"),item("turn","before-1")]} else {vec![item("turn","after")]},"nextCursor":if serial==1 {Some("earlier-page")} else if serial==2 {Some("later-page")} else {None},"backwardsCursor":"reverse-page"})
				};
				let frame = serde_json::json!({"id":request["id"],"result":response});
				write.write_all(format!("{frame}\n").as_bytes()).await.unwrap();
			}
		});
		let context =
			client.thread_item_context("thread", "turn", "anchor", None, false).await.unwrap();
		assert_eq!(
			context["data"]
				.as_array()
				.unwrap()
				.iter()
				.map(|entry| entry["item"]["id"].as_str().unwrap())
				.collect::<Vec<_>>(),
			["before-1", "before-2", "anchor", "after"]
		);
		assert_eq!(context["previousCursor"], "earlier-page");
		assert_eq!(context["nextCursor"], "later-page");
		// A continuation is opaque and does not re-read or re-insert the selected item.
		let continued = client
			.thread_item_context("thread", "turn", "anchor", Some("later-page"), false)
			.await
			.unwrap();
		assert_eq!(continued["data"].as_array().unwrap().len(), 1);
		client.close();
		server.await.unwrap();
	}
}
