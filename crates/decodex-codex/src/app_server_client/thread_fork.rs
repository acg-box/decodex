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
		// Native fork resolves omitted providers from current creation defaults.
		// A reviewed branch must retain the source provider after config changes.
		let provider = self
			.thread_model_settings(source, guard.clone())
			.await
			.map_err(|_| ClientError::StaleHistory)?
			.and_then(|settings| settings.model_provider)
			.ok_or(ClientError::StaleHistory)?;

		params["modelProvider"] = serde_json::json!(provider);

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

	use crate::app_server_client::thread_fork::{
		AppServerClient, ClientError, ThreadForkBoundary, Value,
	};

	#[tokio::test]
	async fn fork_does_not_submit_with_missing_or_changed_source_provider() {
		for changed in [false, true] {
			let (local, remote) = io::duplex(4_096);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/read");

				if changed {
					write.write_all(b"{\"method\":\"thread/settings/updated\",\"params\":{\"threadId\":\"source\",\"threadSettings\":{}}}\n").await.unwrap();
				}

				let provider = changed.then_some("source-provider");

				write.write_all(format!("{}\n", serde_json::json!({"id":request["id"],"result":{"thread":{"id":"source","model":"model","reasoningEffort":null,"modelProvider":provider}}})).as_bytes()).await.unwrap();
				assert!(lines.next_line().await.unwrap().is_none(), "must not submit a fork");
			});
			let result = client
				.fork_thread_at_boundary(
					"source",
					ThreadForkBoundary::BeforeInput("first"),
					client.thread_settings_guard("source").unwrap(),
				)
				.await;

			assert!(matches!(result, Err(ClientError::StaleHistory)));
			client.close();
			server.await.unwrap();
		}
	}

	#[tokio::test]
	async fn fork_selects_one_boundary_and_never_sends_a_turn_or_revert() {
		for before in [true, false] {
			let (local, remote) = io::duplex(65_536);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let read: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(read["method"], "thread/read");
				assert_eq!(
					read["params"],
					serde_json::json!({"threadId":"source","includeTurns":false})
				);
				write.write_all(format!("{}\n", serde_json::json!({"id":read["id"],"result":{"thread":{"id":"source","model":"model","reasoningEffort":null,"modelProvider":"source-provider"}}})).as_bytes()).await.unwrap();

				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/fork");
				assert_eq!(
					request["params"],
					if before {
						serde_json::json!({"threadId":"source","beforeTurnId":"first","deferGoalContinuation":true,"excludeTurns":true,"modelProvider":"source-provider"})
					} else {
						serde_json::json!({"threadId":"source","lastTurnId":"first","deferGoalContinuation":true,"excludeTurns":true,"modelProvider":"source-provider"})
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
