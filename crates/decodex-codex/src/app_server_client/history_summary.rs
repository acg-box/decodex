//! Display-only summaries. Never use these pages to reconcile execution.
use std::{collections::HashSet, time::Duration};

use serde_json::Value;
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError};

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

		time::timeout(Duration::from_secs(10), async {
            let metadata = self.thread_read(serde_json::json!({"threadId":thread})).await?;

            if metadata["thread"]["id"] != thread || metadata["thread"]["historyMode"] != "paginated" {
                return Err(ClientError::InvalidFrame);
            }

            let page = self.request("thread/turns/list", serde_json::json!({"threadId":thread,"cursor":null,"limit":limit,"sortDirection":"desc","itemsView":"summary"})).await?;
            let turns = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;

            if turns.len() > limit as usize { return Err(ClientError::CapacityExceeded); }

            let mut ids = HashSet::new();

            for turn in turns {
                let id = turn["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 512).ok_or(ClientError::InvalidFrame)?;

                if !ids.insert(id) || !turn["items"].is_array() || turn["itemsView"] != "summary" {
                    return Err(ClientError::InvalidFrame);
                }
            }

            Ok(serde_json::json!({"threadId":thread,"turns":turns.iter().rev().collect::<Vec<_>>()}))
        }).await.map_err(|_| ClientError::Io)?
	}
}

#[cfg(test)]
mod tests {
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::app_server_client::history_summary::{AppServerClient, Value};

	#[tokio::test]
	async fn summary_rejects_wrong_identity_legacy_and_incomplete_pages() {
		for (metadata, page) in [
			(serde_json::json!({"id":"wrong","historyMode":"paginated"}), None),
			(serde_json::json!({"id":"thread","historyMode":"legacy"}), None),
			(
				serde_json::json!({"id":"thread","historyMode":"paginated"}),
				Some(serde_json::json!({"data":[{"id":"t","items":[],"itemsView":"notLoaded"}]})),
			),
			(
				serde_json::json!({"id":"thread","historyMode":"paginated"}),
				Some(
					serde_json::json!({"data":[{"id":"t","items":[],"itemsView":"summary"},{"id":"t","items":[],"itemsView":"summary"}]}),
				),
			),
			(
				serde_json::json!({"id":"thread","historyMode":"paginated"}),
				Some(
					serde_json::json!({"data":vec![serde_json::json!({"id":"t","items":[],"itemsView":"summary"});101]}),
				),
			),
		] {
			let (local, remote) = io::duplex(65_536);
			let (reader, writer) = io::split(local);
			let (client, _) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let mut replies = vec![("thread/read", serde_json::json!({"thread":metadata}))];

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
							format!("{}\n", serde_json::json!({"id":request["id"],"result":value}))
								.as_bytes(),
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
		let (local, remote) = io::duplex(65_536);
		let (reader, writer) = io::split(local);
		let (client, _) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = io::split(remote);
			let mut lines = BufReader::new(reader).lines();

			for (method, expected, result) in [
				(
					"thread/read",
					serde_json::json!({"threadId":"thread"}),
					serde_json::json!({"thread":{"id":"thread","historyMode":"paginated"}}),
				),
				(
					"thread/turns/list",
					serde_json::json!({"threadId":"thread","cursor":null,"limit":2,"sortDirection":"desc","itemsView":"summary"}),
					serde_json::json!({"data":[{"id":"new","itemsView":"summary","items":[]},{"id":"old","itemsView":"summary","items":[]}],"nextCursor":"full-history-cursor-must-not-escape"}),
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
						format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
							.as_bytes(),
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
