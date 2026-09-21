//! Retry only proven pre-acceptance closing refusals on the same native connection.
use super::{AppServerClient, ClientError, Value};
use std::time::Duration;

fn closing(error: &ClientError, thread: &str) -> bool {
	matches!(error, ClientError::Remote(error) if error.code == -32600
		&& error.message.starts_with(&format!("thread {thread} is closing;")))
}

pub(super) async fn resume(client: &AppServerClient, params: Value) -> Result<Value, ClientError> {
	let thread = params["threadId"].as_str().filter(|id| !id.is_empty()).map(str::to_owned);
	let revision = client.history_revision();
	let mut result = client.request("thread/resume", params.clone()).await;
	let Some(thread) = thread else { return result };
	for delay in [1, 2, 4, 8] {
		if !result.as_ref().is_err_and(|error| closing(error, &thread)) {
			break;
		}
		tokio::time::sleep(Duration::from_secs(delay)).await;
		let Some(guard) = client.history_guard(revision) else {
			return Err(ClientError::InvalidFrame);
		};
		result = client.request_with_history("thread/resume", params.clone(), guard).await;
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn closing_resume_retries_exact_params_without_submitting_input() {
		let (local, remote) = tokio::io::duplex(4096);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let params = json!({"threadId":"exact-thread","excludeTurns":true});
		let expected = params.clone();
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			for attempt in 0..2 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/resume");
				assert_eq!(request["params"], expected);
				let response = if attempt == 0 {
					json!({"id":request["id"],"error":{"code":-32600,"message":"thread exact-thread is closing; retry after close"}})
				} else {
					json!({"id":request["id"],"result":{"thread":{"id":"exact-thread"}}})
				};
				w.write_all(format!("{response}\n").as_bytes()).await.unwrap();
			}
		});
		assert_eq!(client.thread_resume(params).await.unwrap()["thread"]["id"], "exact-thread");
		server.await.unwrap();
	}

	#[tokio::test]
	async fn resume_does_not_retry_other_refusals_or_lost_responses() {
		for (code, message) in [
			(-32600, "thread other is closing; retry"),
			(-32603, "thread exact-thread is closing; retry"),
			(-32600, "thread exact-thread already has an active writer"),
			(0, ""),
		] {
			let (local, remote) = tokio::io::duplex(4096);
			let (r, w) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let server = tokio::spawn(async move {
				let (r, mut w) = tokio::io::split(remote);
				let mut lines = BufReader::new(r).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				if code == 0 {
					return;
				}
				w.write_all(
					format!(
						"{}\n",
						json!({"id":request["id"],"error":{"code":code,"message":message}})
					)
					.as_bytes(),
				)
				.await
				.unwrap();
				assert!(
					tokio::time::timeout(Duration::from_millis(50), lines.next_line())
						.await
						.is_err()
				);
			});
			let result = client.thread_resume(json!({"threadId":"exact-thread"})).await;
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
			let (local, remote) = tokio::io::duplex(4096);
			let (r, w) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let server = tokio::spawn(async move {
				let (r, mut w) = tokio::io::split(remote);
				let mut lines = BufReader::new(r).lines();
				for _ in 0..if reverted { 1 } else { 5 } {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					assert_eq!(request["method"], "thread/resume");
					w.write_all(format!("{}\n",json!({"id":request["id"],"error":{"code":-32600,"message":"thread exact-thread is closing; retry"}})).as_bytes()).await.unwrap();
				}
				if reverted {
					w.write_all(b"{\"method\":\"thread/reverted\",\"params\":{\"threadId\":\"exact-thread\"}}\n").await.unwrap();
				}
				assert!(
					tokio::time::timeout(
						Duration::from_millis(if reverted { 1200 } else { 50 }),
						lines.next_line()
					)
					.await
					.is_err()
				);
			});
			let result = client.thread_resume(json!({"threadId":"exact-thread"})).await;
			if reverted {
				assert!(matches!(result, Err(ClientError::InvalidFrame)));
			} else {
				assert!(result.as_ref().is_err_and(|e| closing(e, "exact-thread")));
			}
			server.await.unwrap();
		}
	}
}
