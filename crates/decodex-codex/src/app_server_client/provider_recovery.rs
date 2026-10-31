//! Observe native provider refusals without changing configuration or replaying input.
use std::{collections::HashMap, sync::Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AppServerClient, ClientError, RpcError};

/// A native refusal to continue one chat under a different configured provider.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProviderMismatch {
	/// The provider retained by the original chat.
	pub model_provider: String,
	/// The provider selected by the native configuration.
	pub current_model_provider: String,
}

impl NativeProviderMismatch {
	/// Read only the native error discriminator and bounded provider identities.
	pub fn from_error(error: &RpcError) -> Option<Self> {
		let data = error.data.as_ref()?;
		if error.code != -32_600 || data["type"] != "modelProviderMismatch" {
			return None;
		}

		let value: Self = serde_json::from_value(data.clone()).ok()?;
		let valid =
			|id: &str| !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control);

		(valid(&value.model_provider)
			&& valid(&value.current_model_provider)
			&& value.model_provider != value.current_model_provider)
			.then_some(value)
	}
}

#[derive(Default)]
pub(super) struct ProviderMismatches(Mutex<HashMap<String, NativeProviderMismatch>>);

impl ProviderMismatches {
	pub(super) fn observe(&self, thread: &str, result: &Result<Value, RpcError>) {
		let Ok(mut rows) = self.0.lock() else { return };
		let mismatch = result.as_ref().err().and_then(NativeProviderMismatch::from_error);
		if let Some(mismatch) = mismatch {
			rows.insert(thread.into(), mismatch);
		} else {
			rows.remove(thread);
		}
	}

	pub(super) fn get(&self, thread: &str) -> Option<NativeProviderMismatch> {
		self.0.lock().ok()?.get(thread).cloned()
	}

	pub(super) fn clear(&self) {
		if let Ok(mut rows) = self.0.lock() {
			rows.clear();
		}
	}

	pub(super) fn clear_if(&self, thread: &str, expected: &NativeProviderMismatch) {
		if let Ok(mut rows) = self.0.lock()
			&& rows.get(thread) == Some(expected)
		{
			rows.remove(thread);
		}
	}
}

impl AppServerClient {
	/// Recheck the configured route without resuming a chat or sending input.
	pub async fn refresh_provider_mismatch(
		&self,
		thread: &str,
	) -> Result<Option<NativeProviderMismatch>, ClientError> {
		let Some(mismatch) = self.provider_mismatch(thread) else { return Ok(None) };
		if !self.provider_matches_configuration(thread, &mismatch.current_model_provider).await? {
			self.server_requests.clear_provider_mismatch(thread, &mismatch);
		}
		Ok(self.provider_mismatch(thread))
	}

	pub(super) async fn provider_matches_configuration(
		&self,
		thread: &str,
		provider: &str,
	) -> Result<bool, ClientError> {
		tokio::time::timeout(std::time::Duration::from_secs(8), async {
			let metadata = self.thread_read(serde_json::json!({"threadId":thread})).await?;
			let cwd = metadata["thread"]["cwd"].as_str().ok_or(ClientError::InvalidFrame)?;
			if metadata["thread"]["id"] != thread {
				return Err(ClientError::InvalidFrame);
			}
			let configured = self
				.request("config/read", serde_json::json!({"cwd":cwd,"includeLayers":false}))
				.await?;
			let config = configured["config"].as_object().ok_or(ClientError::InvalidFrame)?;
			let selected = match config.get("model_provider") {
				None | Some(Value::Null) => "openai",
				Some(Value::String(value)) => value.as_str(),
				_ => return Err(ClientError::InvalidFrame),
			};
			Ok(selected == provider)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Only a structured refusal observed on this live connection permits provider recovery.
	pub fn provider_mismatch(&self, thread: &str) -> Option<NativeProviderMismatch> {
		if *self.closed.borrow() || self.outbound.is_closed() {
			return None;
		}

		self.server_requests.provider_mismatch(thread)
	}
}

#[cfg(test)]
mod tests {
	use super::{AppServerClient, NativeProviderMismatch};
	use serde_json::{Value, json};
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	#[tokio::test]
	async fn provider_recovery_requires_exact_native_refusal_and_clears_after_resume() {
		let (local, remote) = io::duplex(16_384);
		let (read, write) = io::split(local);
		let (client, _events) = AppServerClient::from_io(read, write);
		let server = tokio::spawn(async move {
			let (read, mut write) = io::split(remote);
			let mut lines = BufReader::new(read).lines();
			for data in [
				json!({"type":"otherError","modelProvider":"old","currentModelProvider":"new"}),
				json!({"type":"modelProviderMismatch","modelProvider":"old","currentModelProvider":"new"}),
				Value::Null,
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/resume");
				let response = if data.is_null() {
					json!({"id":request["id"],"result":{"thread":{"id":"source"}}})
				} else {
					json!({"id":request["id"],"error":{"code":-32600,"message":"private context is not a recovery signal","data":data}})
				};
				write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
			}
		});
		assert!(client.thread_resume(json!({"threadId":"source"})).await.is_err());
		assert_eq!(client.provider_mismatch("source"), None);
		assert!(client.thread_resume(json!({"threadId":"source"})).await.is_err());
		assert_eq!(
			client.provider_mismatch("source"),
			Some(NativeProviderMismatch {
				model_provider: "old".into(),
				current_model_provider: "new".into()
			})
		);
		assert_eq!(client.provider_mismatch("unrelated"), None);
		client.thread_resume(json!({"threadId":"source"})).await.unwrap();
		assert_eq!(client.provider_mismatch("source"), None);
		client.close();
		server.await.unwrap();
	}
}
