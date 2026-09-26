//! Display-only summaries. Never use these pages to reconcile execution.
use super::{AppServerClient, ClientError};
use serde_json::{Value, json};
use std::collections::HashSet;

impl AppServerClient {
	/// Read recent paginated turns in chronological order without obtaining a writer lease.
	/// The result is incomplete display content, with no usable full-history cursor.
	pub async fn thread_history_summary(
		&self,
		thread: &str,
		limit: u32,
	) -> Result<Value, ClientError> {
		if thread.is_empty() || thread.len() > 512 || !(1..=100).contains(&limit) {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let metadata = self.thread_read(json!({"threadId":thread})).await?;
            if metadata["thread"]["id"] != thread || metadata["thread"]["historyMode"] != "paginated" {
                return Err(ClientError::InvalidFrame);
            }
            let page = self.request("thread/turns/list", json!({"threadId":thread,"cursor":null,"limit":limit,"sortDirection":"desc","itemsView":"summary"})).await?;
            let turns = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
            if turns.len() > limit as usize { return Err(ClientError::CapacityExceeded); }
            let mut ids = HashSet::new();
            for turn in turns {
                let id = turn["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 512).ok_or(ClientError::InvalidFrame)?;
                if !ids.insert(id) || !turn["items"].is_array() || turn["itemsView"] != "summary" {
                    return Err(ClientError::InvalidFrame);
                }
            }
            Ok(json!({"threadId":thread,"turns":turns.iter().rev().collect::<Vec<_>>()}))
        }).await.map_err(|_| ClientError::Io)?
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn summary_rejects_wrong_identity_legacy_and_incomplete_pages() {
		for (metadata, page) in [
			(json!({"id":"wrong","historyMode":"paginated"}), None),
			(json!({"id":"thread","historyMode":"legacy"}), None),
			(
				json!({"id":"thread","historyMode":"paginated"}),
				Some(json!({"data":[{"id":"t","items":[],"itemsView":"notLoaded"}]})),
			),
			(
				json!({"id":"thread","historyMode":"paginated"}),
				Some(
					json!({"data":[{"id":"t","items":[],"itemsView":"summary"},{"id":"t","items":[],"itemsView":"summary"}]}),
				),
			),
			(
				json!({"id":"thread","historyMode":"paginated"}),
				Some(json!({"data":vec![json!({"id":"t","items":[],"itemsView":"summary"});101]})),
			),
		] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let mut replies = vec![("thread/read", json!({"thread":metadata}))];
				if let Some(page) = page {
					replies.push(("thread/turns/list", page));
				}
				for (method, value) in replies {
					let request: Value = serde_json::from_str(
						&lines.next_line().await.expect("read").expect("request"),
					)
					.expect("JSON");
					assert_eq!(request["method"], method);
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":value})).as_bytes(),
						)
						.await
						.expect("reply");
				}
			});
			assert!(client.thread_history_summary("thread", 100).await.is_err());
			server.await.expect("fixture");
		}
	}
	#[tokio::test]
	async fn summary_is_chronological_display_content_without_writer_or_full_cursor() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for (method, expected, result) in [
				(
					"thread/read",
					json!({"threadId":"thread"}),
					json!({"thread":{"id":"thread","historyMode":"paginated"}}),
				),
				(
					"thread/turns/list",
					json!({"threadId":"thread","cursor":null,"limit":2,"sortDirection":"desc","itemsView":"summary"}),
					json!({"data":[{"id":"new","itemsView":"summary","items":[]},{"id":"old","itemsView":"summary","items":[]}],"nextCursor":"full-history-cursor-must-not-escape"}),
				),
			] {
				let request: Value = serde_json::from_str(
					&lines.next_line().await.expect("read request").expect("request"),
				)
				.expect("request JSON");
				assert_eq!(request["method"], method);
				assert_eq!(request["params"], expected);
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.expect("response");
			}
		});
		let result = client.thread_history_summary("thread", 2).await.expect("summary");
		assert_eq!(result["threadId"], "thread");
		assert_eq!(result["turns"][0]["id"], "old");
		assert_eq!(result["turns"][1]["id"], "new");
		assert!(result.get("nextCursor").is_none());
		server.await.expect("read-only fixture");
	}
}
