//! Retry only proven pre-acceptance closing refusals on the same native connection.
use std::time::Duration;

use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError, Value};

pub(super) async fn resume(client: &AppServerClient, params: Value) -> Result<Value, ClientError> {
	let thread = params["threadId"].as_str().filter(|id| !id.is_empty()).map(str::to_owned);
	let revision = client.history_revision();
	let mut result = client.request("thread/resume", params.clone()).await;
	let Some(thread) = thread else { return result };

	for delay in [1, 2, 4, 8] {
		if !result.as_ref().is_err_and(|error| closing(error, &thread)) {
			break;
		}

		time::sleep(Duration::from_secs(delay)).await;

		let Some(guard) = client.history_guard(revision) else {
			return Err(ClientError::InvalidFrame);
		};

		result = client.request_with_history("thread/resume", params.clone(), guard).await;
	}

	result
}

fn closing(error: &ClientError, thread: &str) -> bool {
	matches!(error, ClientError::Remote(error) if error.code == -32_600
		&& error.message.starts_with(&format!("thread {thread} is closing;")))
}

#[cfg(test)]
mod tests {
	use serde_json;
	use tokio::{
		io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
		time,
	};

	use crate::app_server_client::resume::{self, AppServerClient, ClientError, Duration, Value};

	#[tokio::test]
	async fn pending_resume_allows_peer_metadata_and_does_not_retry_removed_thread() {
		let (local, remote) = io::duplex(4_096);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let (r, mut w) = io::split(remote);
		let mut lines = BufReader::new(r).lines();
		let resuming = client.clone();
		let resume = tokio::spawn(async move {
			resuming.thread_resume(serde_json::json!({"threadId":"cold","excludeTurns":true})).await
		});
		let first: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

		assert_eq!(first["method"], "thread/resume");

		let peer = client.clone();
		let update = tokio::spawn(async move {
			peer.request("thread/name/set", serde_json::json!({"threadId":"peer","name":"Renamed"}))
				.await
		});
		let next = time::timeout(Duration::from_secs(2), lines.next_line())
			.await
			.unwrap()
			.unwrap()
			.unwrap();
		let next: Value = serde_json::from_str(&next).unwrap();

		assert_eq!(next["method"], "thread/name/set");
		assert_ne!(first["id"], next["id"]);

		w.write_all(format!("{}\n", serde_json::json!({"id":next["id"],"result":{}})).as_bytes())
			.await
			.unwrap();

		time::timeout(Duration::from_secs(2), update).await.unwrap().unwrap().unwrap();

		assert!(!resume.is_finished(), "peer response must not complete the pending resume");

		w.write_all(
			format!(
				"{}\n",
				serde_json::json!({"id":first["id"],"error":{"code":-32_600,"message":"thread cold not found"}})
			)
			.as_bytes(),
		)
		.await
		.unwrap();

		assert!(
			matches!(resume.await.unwrap(), Err(ClientError::Remote(error)) if error.code == -32_600)
		);
		assert!(time::timeout(Duration::from_millis(50), lines.next_line()).await.is_err());

		client.shutdown().await.unwrap();
	}

	#[tokio::test]
	async fn closing_resume_retries_exact_params_without_submitting_input() {
		let (local, remote) = io::duplex(4_096);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let params = serde_json::json!({"threadId":"exact-thread","excludeTurns":true});
		let expected = params.clone();
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();

			for attempt in 0..2 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/resume");
				assert_eq!(request["params"], expected);

				let response = if attempt == 0 {
					serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"thread exact-thread is closing; retry after close"}})
				} else {
					serde_json::json!({"id":request["id"],"result":{"thread":{"id":"exact-thread"}}})
				};

				w.write_all(format!("{response}\n").as_bytes()).await.unwrap();

				if attempt == 0 {
					w.write_all(b"{\"method\":\"thread/closed\",\"params\":{\"threadId\":\"exact-thread\"}}\n").await.unwrap();
				}
			}
		});

		assert_eq!(client.thread_resume(params).await.unwrap()["thread"]["id"], "exact-thread");

		server.await.unwrap();
	}

	#[tokio::test]
	async fn resume_does_not_retry_other_refusals_or_lost_responses() {
		for (code, message) in [
			(-32_600, "thread other is closing; retry"),
			(-32_603, "thread exact-thread is closing; retry"),
			(-32_600, "thread exact-thread already has an active writer"),
			(0, ""),
		] {
			let (local, remote) = io::duplex(4_096);
			let (r, w) = io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let server = tokio::spawn(async move {
				let (r, mut w) = io::split(remote);
				let mut lines = BufReader::new(r).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				if code == 0 {
					return;
				}

				w.write_all(
					format!(
						"{}\n",
						serde_json::json!({"id":request["id"],"error":{"code":code,"message":message}})
					)
					.as_bytes(),
				)
				.await
				.unwrap();

				assert!(time::timeout(Duration::from_millis(50), lines.next_line()).await.is_err());
			});
			let result = client.thread_resume(serde_json::json!({"threadId":"exact-thread"})).await;

			if code == 0 {
				assert!(result.is_err());
			} else {
				assert!(matches!(result, Err(ClientError::Remote(error))
					if error.code == code && error.message == message));
			}

			server.await.unwrap();
		}
	}
	#[tokio::test]
	async fn closing_retry_is_bounded_and_stops_after_revert() {
		for reverted in [false, true] {
			let (local, remote) = io::duplex(4_096);
			let (r, w) = io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let server = tokio::spawn(async move {
				let (r, mut w) = io::split(remote);
				let mut lines = BufReader::new(r).lines();

				for _ in 0..if reverted { 1 } else { 5 } {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

					assert_eq!(request["method"], "thread/resume");

					w.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"thread exact-thread is closing; retry"}})).as_bytes()).await.unwrap();
				}

				if reverted {
					w.write_all(b"{\"method\":\"thread/reverted\",\"params\":{\"threadId\":\"exact-thread\"}}\n").await.unwrap();
				}

				assert!(
					time::timeout(
						Duration::from_millis(if reverted { 1_200 } else { 50 }),
						lines.next_line()
					)
					.await
					.is_err()
				);
			});
			let result = client.thread_resume(serde_json::json!({"threadId":"exact-thread"})).await;

			if reverted {
				assert!(matches!(result, Err(ClientError::InvalidFrame)));
			} else {
				assert!(result.as_ref().is_err_and(|e| resume::closing(e, "exact-thread")));
			}

			server.await.unwrap();
		}
	}
}
