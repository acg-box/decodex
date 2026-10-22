//! One native response without tool execution. Native owns the temporary child and cleanup.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{sync::oneshot, time};

use super::{AppServerClient, ClientError, Guard, HistoryGuard, Outbound, RequestId};

/// Maximum prompt size accepted by the selected native sampling interface.
pub const MAX_SAMPLE_PROMPT_BYTES: usize = 4_000;
/// Maximum assistant text returned by native sampling.
pub const MAX_SAMPLE_OUTPUT_BYTES: usize = 64 * 1_024;

/// A single response from an inherited temporary native child.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeThreadSample {
	/// Latest finished source turn checked by the native server.
	pub source_turn_id: String,
	/// Exact model response identity.
	pub response_id: String,
	/// Absent when the model attempted a tool call or returned no assistant text.
	pub text: Option<String>,
	/// Native input usage, when reported.
	pub input_tokens: Option<i64>,
	/// Native cached input usage, when reported.
	pub cached_input_tokens: Option<i64>,
	/// Native output usage, when reported.
	pub output_tokens: Option<i64>,
}

/// Native sampling completion, with cancellation confirmed by the original response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeThreadSampleOutcome {
	/// A normal model response won any race with cancellation.
	Answer(NativeThreadSample),
	/// Native stopped the exact sampling request and finished child cleanup.
	Cancelled,
}

fn identity(value: &str) -> bool {
	!value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

/// Admit only explicit inherited sampling, with no configuration overrides.
pub fn is_thread_sample(params: &Value) -> bool {
	params.as_object().is_some_and(|p| p.len() == 3)
		&& params["threadId"].as_str().is_some_and(identity)
		&& params["expectedTurnId"].as_str().is_some_and(identity)
		&& params["prompt"]
			.as_str()
			.is_some_and(|text| !text.trim().is_empty() && text.len() <= MAX_SAMPLE_PROMPT_BYTES)
}

/// Admit only a bounded original request identity, with no thread or turn mutation.
pub fn is_thread_sample_cancel(params: &Value) -> bool {
	params.as_object().is_some_and(|p| p.len() == 1)
		&& serde_json::from_value::<RequestId>(params["requestId"].clone()).is_ok_and(|id| match id
		{
			RequestId::Number(_) => true,
			RequestId::String(id) => identity(&id),
		})
}

impl AppServerClient {
	/// Request one answer without submitting input to the parent or executing tools.
	/// Native requires an idle persistent root and its latest finished turn.
	/// A timeout or lost reply is never retried or replaced with a tool-enabled fork.
	pub async fn sample_thread(
		&self,
		thread: &str,
		expected_turn: &str,
		prompt: &str,
		guard: HistoryGuard,
	) -> Result<NativeThreadSample, ClientError> {
		let (_keep_open, cancellation) = oneshot::channel();
		match self
			.sample_thread_cancellable(thread, expected_turn, prompt, guard, cancellation)
			.await?
		{
			NativeThreadSampleOutcome::Answer(answer) => Ok(answer),
			NativeThreadSampleOutcome::Cancelled => Err(ClientError::InvalidFrame),
		}
	}

	/// Cancel only this written request on its original connection, and await native cleanup.
	/// Dropping the cancellation sender leaves inference running. A completed answer wins.
	pub async fn sample_thread_cancellable(
		&self,
		thread: &str,
		expected_turn: &str,
		prompt: &str,
		guard: HistoryGuard,
		cancellation: oneshot::Receiver<()>,
	) -> Result<NativeThreadSampleOutcome, ClientError> {
		let params =
			serde_json::json!({"threadId":thread,"expectedTurnId":expected_turn,"prompt":prompt});
		if !is_thread_sample(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let guard =
			self.with_thread_settings_guard(thread, guard).ok_or(ClientError::StaleHistory)?;
		let result = time::timeout(
			Duration::from_secs(35),
			self.sample_response(params, guard.clone(), cancellation),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let Some(result) = result else { return Ok(NativeThreadSampleOutcome::Cancelled) };
		let result: NativeThreadSample =
			serde_json::from_value(result).map_err(|_| ClientError::InvalidFrame)?;
		if !guard.is_live() {
			return Err(ClientError::StaleHistory);
		}
		if result.source_turn_id != expected_turn
			|| !identity(&result.response_id)
			|| result.text.as_ref().is_some_and(|text| text.len() > MAX_SAMPLE_OUTPUT_BYTES)
			|| [result.input_tokens, result.cached_input_tokens, result.output_tokens]
				.iter()
				.flatten()
				.any(|value| *value < 0)
		{
			return Err(ClientError::InvalidFrame);
		}
		Ok(NativeThreadSampleOutcome::Answer(result))
	}

	async fn sample_response(
		&self,
		params: Value,
		guard: HistoryGuard,
		cancellation: oneshot::Receiver<()>,
	) -> Result<Option<Value>, ClientError> {
		let (reply, mut response) = oneshot::channel();
		let (written, receipt) = oneshot::channel();
		self.outbound
			.send(Outbound::Request {
				method: "thread/sample".into(),
				params,
				reply,
				guard: Some(Guard::History(guard)),
				written: Some(written),
			})
			.await
			.map_err(|_| ClientError::Closed)?;
		// Await the transport identity even when cancellation arrives during admission.
		let id = match receipt.await {
			Ok(id) => id,
			Err(_) =>
				return Err(response
					.await
					.unwrap_or(Err(ClientError::Closed))
					.err()
					.unwrap_or(ClientError::InvalidFrame)),
		};
		let mut requested = false;
		let result = tokio::select! {
			biased;
			result = &mut response => result.unwrap_or(Err(ClientError::Closed)),
			cancel = cancellation => {
				if cancel.is_err() { return response.await.unwrap_or(Err(ClientError::Closed)).map(Some); }
				requested = true;
				let cancel = self.request("thread/sample/cancel", serde_json::json!({"requestId":id}));
				tokio::pin!(cancel);
				tokio::select! {
					biased;
					result = &mut response => result.unwrap_or(Err(ClientError::Closed)),
					_ = &mut cancel => response.await.unwrap_or(Err(ClientError::Closed)),
				}
			}
		};
		// An acknowledgement only signals cancellation; the original reply follows native cleanup.
		if requested
			&& matches!(&result, Err(ClientError::Remote(e)) if e.code == -32_603 && e.message == "sample canceled")
		{
			Ok(None)
		} else {
			result.map(Some)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	#[tokio::test]
	async fn sampling_sends_one_native_request_and_preserves_null_tool_results() {
		for text in [Value::Null, serde_json::json!("A second approach.")] {
			let (local, remote) = io::duplex(8_192);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let guard = client.history_guard(client.history_revision()).unwrap();
			let expected = text.clone();
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/sample");
				assert_eq!(
					request["params"],
					serde_json::json!({"threadId":"parent","expectedTurnId":"done","prompt":"Another approach?"})
				);
				write.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"result":{"sourceTurnId":"done","responseId":"response","text":text,"inputTokens":12,"cachedInputTokens":4,"outputTokens":3}})).as_bytes()).await.unwrap();
				assert!(time::timeout(Duration::from_millis(25), lines.next_line()).await.is_err());
			});
			let result =
				client.sample_thread("parent", "done", "Another approach?", guard).await.unwrap();
			assert_eq!(result.text.as_deref(), expected.as_str());
			assert_eq!(result.input_tokens, Some(12));
			server.await.unwrap();
		}
	}

	#[tokio::test]
	async fn sampling_rejects_a_response_for_a_different_source_turn() {
		let (local, remote) = io::duplex(8_192);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let guard = client.history_guard(client.history_revision()).unwrap();
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let request: Value = serde_json::from_str(
				&BufReader::new(read).lines().next_line().await.unwrap().unwrap(),
			)
			.unwrap();
			write.write_all(format!("{}\n",serde_json::json!({"id":request["id"],"result":{"sourceTurnId":"other","responseId":"response","text":"Wrong history"}})).as_bytes()).await.unwrap();
			time::sleep(Duration::from_millis(25)).await;
		});
		assert!(matches!(
			client.sample_thread("parent", "done", "Ask", guard).await,
			Err(ClientError::InvalidFrame)
		));
		server.await.unwrap();
	}
	#[tokio::test]
	async fn sampling_cancel_waits_for_the_original_cleanup_response() {
		let (local, remote) = io::duplex(8192);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let guard = client.history_guard(client.history_revision()).unwrap();
		let (cancel, cancellation) = oneshot::channel();
		let (ack, acknowledged) = oneshot::channel();
		let (cleanup, released) = oneshot::channel();
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();
			let original: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			cancel.send(()).unwrap();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "thread/sample/cancel");
			assert_eq!(request["params"], serde_json::json!({"requestId":original["id"]}));
			write
				.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":{}})).as_bytes(),
				)
				.await
				.unwrap();
			ack.send(()).unwrap();
			released.await.unwrap();
			write
				.write_all(
					format!(
						"{}\n",
						serde_json::json!({"id":original["id"],"error":{"code":-32603,"message":"sample canceled"}})
					)
					.as_bytes(),
				)
				.await
				.unwrap();
		});
		let task = tokio::spawn(async move {
			client.sample_thread_cancellable("parent", "done", "Ask", guard, cancellation).await
		});
		acknowledged.await.unwrap();
		assert!(!task.is_finished(), "an acknowledgement does not prove child cleanup");
		cleanup.send(()).unwrap();
		assert_eq!(task.await.unwrap().unwrap(), NativeThreadSampleOutcome::Cancelled);
		server.await.unwrap();
	}

	#[tokio::test]
	async fn sampling_cancel_races_and_refusal_preserve_completed_answers() {
		for refused in [false, true] {
			let (local, remote) = io::duplex(8192);
			let (read, write) = io::split(local);
			let (client, _events) = AppServerClient::from_io(read, write);
			let guard = client.history_guard(client.history_revision()).unwrap();
			let (cancel, cancellation) = oneshot::channel();
			let server = tokio::spawn(async move {
				let (read, mut write) = io::split(remote);
				let mut lines = BufReader::new(read).lines();
				let original: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				cancel.send(()).unwrap();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/sample/cancel");
				assert_eq!(request["params"]["requestId"], original["id"]);
				let ack = if refused {
					serde_json::json!({"id":request["id"],"error":{"code":-32601,"message":"unsupported"}})
				} else {
					serde_json::json!({"id":request["id"],"result":{}})
				};
				write.write_all(format!("{ack}\n").as_bytes()).await.unwrap();
				write.write_all(format!("{}\n", serde_json::json!({"id":original["id"],"result":{"sourceTurnId":"done","responseId":"finished","text":"Completed before cancellation"}})).as_bytes()).await.unwrap();
				assert!(time::timeout(Duration::from_millis(25), lines.next_line()).await.is_err());
			});
			let result = client
				.sample_thread_cancellable("parent", "done", "Ask", guard, cancellation)
				.await
				.unwrap();
			assert!(
				matches!(result, NativeThreadSampleOutcome::Answer(answer) if answer.response_id == "finished")
			);
			server.await.unwrap();
		}
	}
}
