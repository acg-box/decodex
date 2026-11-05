//! Exact native item lookup for detail views.
use std::time::Duration;

use serde_json::Value;
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError};

impl AppServerClient {
	/// Read one item in its exact thread and turn. Missing items remain absent.
	pub async fn thread_read_item(
		&self,
		thread: &str,
		turn: &str,
		item: &str,
	) -> Result<Option<Value>, ClientError> {
		if [thread, turn, item]
			.iter()
			.any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
		{
			return Err(ClientError::InvalidFrame);
		}

		time::timeout(Duration::from_secs(20), async {
			let value = self
				.request(
					"thread/items/read",
					serde_json::json!({"threadId":thread,"turnId":turn,"itemIds":[item]}),
				)
				.await?;
			let data = value["data"].as_array().ok_or(ClientError::InvalidFrame)?;

			match data.as_slice() {
				[] => Ok(None),
				[entry] if entry["turnId"] == turn && entry["item"]["id"] == item =>
					Ok(Some(entry["item"].clone())),
				_ => Err(ClientError::InvalidFrame),
			}
		})
		.await
		.map_err(|_| ClientError::Io)?
	}
}

#[cfg(test)]
mod tests {
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::app_server_client::history_item::{AppServerClient, Value};

	#[tokio::test]
	async fn exact_item_uses_one_request_and_rejects_neighbors() {
		for (data, valid, found) in [
			(serde_json::json!([]), true, false),
			(
				serde_json::json!([{"turnId":"turn","item":{"id":"item","type":"fileChange"}}]),
				true,
				true,
			),
			(serde_json::json!([{"turnId":"other","item":{"id":"item"}}]), false, false),
			(serde_json::json!([{"turnId":"turn","item":{"id":"other"}}]), false, false),
			(
				serde_json::json!([{"turnId":"turn","item":{"id":"item"}},{"turnId":"turn","item":{"id":"item"}}]),
				false,
				false,
			),
		] {
			let (local, remote) = io::duplex(8_192);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/items/read");
				assert_eq!(
					request["params"],
					serde_json::json!({"threadId":"thread","turnId":"turn","itemIds":["item"]})
				);

				write
					.write_all(
						format!(
							"{}\n",
							serde_json::json!({"id":request["id"],"result":{"data":data}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();

				assert!(lines.next_line().await.unwrap().is_none());
			});
			let result = client.thread_read_item("thread", "turn", "item").await;

			assert_eq!(result.is_ok(), valid);

			if valid {
				assert_eq!(result.unwrap().is_some(), found);
			}

			client.close();
			server.await.unwrap();
		}
	}
}
