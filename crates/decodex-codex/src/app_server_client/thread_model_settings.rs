//! Read configured thread settings without resuming or dispatching work.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Native configured settings, not the model used by an individual turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeThreadModelSettings {
	/// Current loaded setting or latest persisted model. Null means unavailable.
	pub model: Option<String>,
	/// Native effort spelling, including future levels. Null means unset or unavailable.
	pub reasoning_effort: Option<String>,
}

fn valid(value: &str, limit: usize) -> bool {
	!value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn project(value: Value, expected: &str) -> Result<Option<NativeThreadModelSettings>, ClientError> {
	let thread = value.get("thread").ok_or(ClientError::InvalidFrame)?;
	if thread["id"].as_str() != Some(expected) {
		return Err(ClientError::InvalidFrame);
	}
	if thread.get("model").is_none() || thread.get("reasoningEffort").is_none() {
		return Ok(None);
	}
	let settings: NativeThreadModelSettings =
		serde_json::from_value(thread.clone()).map_err(|_| ClientError::InvalidFrame)?;
	if settings.model.as_deref().is_some_and(|v| !valid(v, 512))
		|| settings.reasoning_effort.as_deref().is_some_and(|v| !valid(v, 128))
	{
		return Err(ClientError::InvalidFrame);
	}
	Ok(Some(settings))
}

impl AppServerClient {
	/// Read one exact thread without activation. Missing fields indicate an older server.
	/// The caller must verify account/process ownership before and after this read.
	pub async fn thread_model_settings(
		&self,
		thread: &str,
		guard: HistoryGuard,
	) -> Result<Option<NativeThreadModelSettings>, ClientError> {
		if !valid(thread, 512) {
			return Err(ClientError::InvalidFrame);
		}
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request_with_history(
				"thread/read",
				json!({"threadId":thread,"includeTurns":false}),
				guard,
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		project(response, thread)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[test]
	fn nullable_settings_are_distinct_from_missing_or_invalid_metadata() {
		for (model, effort) in
			[(Value::Null, Value::Null), (json!("future-model"), json!("future-effort"))]
		{
			let settings =
				project(json!({"thread":{"id":"t","model":model,"reasoningEffort":effort}}), "t")
					.unwrap()
					.unwrap();
			assert_eq!(
				serde_json::to_value(settings).unwrap(),
				json!({"model":model,"reasoningEffort":effort})
			);
		}
		assert!(project(json!({"thread":{"id":"t"}}), "t").unwrap().is_none());
		for thread in [
			json!({"id":"other"}),
			json!({"id":"t","model":42,"reasoningEffort":null}),
			json!({"id":"t","model":"ok","reasoningEffort":"\n"}),
		] {
			assert!(project(json!({"thread":thread}), "t").is_err());
		}
	}

	#[tokio::test]
	async fn settings_read_uses_only_exact_thread_read() {
		let (local, remote) = tokio::io::duplex(4096);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let guard = client.history_guard(0).unwrap();
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "thread/read");
			assert_eq!(request["params"], json!({"threadId":"t","includeTurns":false}));
			w.write_all(format!("{}\n",json!({"id":request["id"],"result":{"thread":{"id":"t","model":"configured","reasoningEffort":null,"turns":[]}}})).as_bytes()).await.unwrap();
		});
		let settings = client.thread_model_settings("t", guard).await.unwrap().unwrap();
		assert_eq!(settings.model.as_deref(), Some("configured"));
		assert_eq!(settings.reasoning_effort, None);
		server.await.unwrap();
	}
}
