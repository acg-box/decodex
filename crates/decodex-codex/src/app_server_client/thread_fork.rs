//! A single source-bound native fork. The caller owns intent and reply recovery.

use serde_json::Value;

use crate::app_server_client::{AppServerClient, ClientError, HistoryGuard};

/// Select the exact persisted prefix of a source conversation.
pub enum ThreadForkBoundary<'a> {
	/// Exclude the selected input turn. The first turn produces an empty native fork.
	BeforeInput(&'a str),
	/// Include the selected completed turn.
	AfterTurn(&'a str),
}

impl AppServerClient {
	/// Submit once after durable reservation. This never reverts the source or submits input.
	/// Persist the acknowledged identity before subsequent history reads can fail.
	pub async fn fork_thread_at_boundary(
		&self,
		source: &str,
		boundary: ThreadForkBoundary<'_>,
		guard: HistoryGuard,
	) -> Result<Value, ClientError> {
		let (field, turn) = match boundary {
			ThreadForkBoundary::BeforeInput(turn) => ("beforeTurnId", turn),
			ThreadForkBoundary::AfterTurn(turn) => ("lastTurnId", turn),
		};

		if [source, turn]
			.iter()
			.any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
		{
			return Err(ClientError::InvalidFrame);
		}

		let mut params =
			serde_json::json!({"threadId":source,"deferGoalContinuation":true,"excludeTurns":true});

		params[field] = serde_json::json!(turn);

		let guard =
			self.with_thread_settings_guard(source, guard).ok_or(ClientError::StaleHistory)?;
		let response = self.request_with_history("thread/fork", params, guard).await?;
		let thread = &response["thread"];

		if thread["forkedFromId"] != source
			|| thread["id"].as_str().is_none_or(|id| {
				id.is_empty() || id.len() > 512 || id == source || id.chars().any(char::is_control)
			}) {
			return Err(ClientError::InvalidFrame);
		}

		Ok(response)
	}
}

#[cfg(test)]
mod tests {
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	use crate::app_server_client::thread_fork::{AppServerClient, ThreadForkBoundary, Value};

	#[tokio::test]
	async fn fork_selects_one_boundary_and_never_sends_a_turn_or_revert() {
		for before in [true, false] {
			let (local, remote) = io::duplex(65_536);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/fork");
				assert_eq!(
					request["params"],
					if before {
						serde_json::json!({"threadId":"source","beforeTurnId":"first","deferGoalContinuation":true,"excludeTurns":true})
					} else {
						serde_json::json!({"threadId":"source","lastTurnId":"first","deferGoalContinuation":true,"excludeTurns":true})
					}
				);

				write
					.write_all(
						format!(
							"{}\n",
							serde_json::json!({"id":request["id"],"result":{"thread":{"id":"branch","forkedFromId":"source"}}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();

				assert!(lines.next_line().await.unwrap().is_none());
			});
			let guard = client.thread_settings_guard("source").unwrap();
			let boundary = if before {
				ThreadForkBoundary::BeforeInput("first")
			} else {
				ThreadForkBoundary::AfterTurn("first")
			};

			assert_eq!(
				client.fork_thread_at_boundary("source", boundary, guard).await.unwrap()["thread"]
					["id"],
				"branch"
			);

			client.close();
			server.await.unwrap();
		}
	}
}
