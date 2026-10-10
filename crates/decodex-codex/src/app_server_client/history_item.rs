//! Exact item lookup for detail views, with an explicit older-server fallback.
use crate::app_server_client::{AppServerClient, ClientError};
use serde_json::Value;
use std::time::Duration;
use tokio::time;

impl AppServerClient {
	/// Read one item in its exact thread and turn. Missing items remain absent.
	/// Only an explicit unsupported-method response permits paginated history fallback.
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
			let result = self
				.request(
					"thread/items/read",
					serde_json::json!({"threadId":thread,"turnId":turn,"itemIds":[item]}),
				)
				.await;
			let value = match result {
				Ok(value) => value,
				Err(ClientError::Remote(error))
					if error.code == -32_601
						|| (error.code == -32_600
							&& error.message.starts_with(
								"Invalid request: unknown variant `thread/items/read`,",
							)) =>
				{
					let history = self.thread_read_turn(thread, turn).await?;
					if history["thread"]["id"] != thread {
						return Err(ClientError::InvalidFrame);
					}
					let turns =
						history["thread"]["turns"].as_array().ok_or(ClientError::InvalidFrame)?;
					let Some(target) = turns.iter().find(|value| value["id"] == turn) else {
						return Ok(None);
					};
					return Ok(target["items"]
						.as_array()
						.ok_or(ClientError::InvalidFrame)?
						.iter()
						.find(|value| value["id"] == item)
						.cloned());
				},
				Err(error) => return Err(error),
			};
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
	use super::*;
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

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
			let (local, remote) = io::duplex(8192);
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
	#[tokio::test]
	async fn exact_item_falls_back_only_for_explicit_unsupported_methods() {
		for (code, message, fallback) in [
			(-32601, "unsupported", true),
			(
				-32600,
				"Invalid request: unknown variant `thread/items/read`, expected known methods",
				true,
			),
			(-32600, "invalid thread", false),
		] {
			let (local, remote) = io::duplex(8192);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/items/read");
				write
					.write_all(
						format!(
							"{}\n",
							serde_json::json!({"id":request["id"],"error":{"code":code,"message":message}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
				if fallback {
					for full in [false, true] {
						let request: Value =
							serde_json::from_str(&lines.next_line().await.unwrap().unwrap())
								.unwrap();
						assert_eq!(request["method"], "thread/read");
						assert_eq!(request["params"]["includeTurns"] == true, full);
						write.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"result":{"thread":{"id":"thread","turns":[{"id":"turn","items":[{"id":"neighbor"},{"id":"item","type":"fileChange"}]}]}}})).as_bytes()).await.unwrap();
					}
				}
				assert!(lines.next_line().await.unwrap().is_none());
			});
			let result = client.thread_read_item("thread", "turn", "item").await;
			assert_eq!(result.is_ok(), fallback);
			if fallback {
				assert_eq!(result.unwrap().unwrap()["id"], "item");
			}
			client.close();
			server.await.unwrap();
		}
	}
}
