//! Read the loaded native conversation's selected memory version without resuming it.
use serde_json::Value;

use crate::app_server_client::{AppServerClient, ClientError};

/// Maximum memory summary accepted by the read-only desktop viewer.
pub const MAX_MEMORY_SUMMARY_BYTES: usize = 1_024 * 1_024;

impl AppServerClient {
	/// Native `thread/memory/read` selects the version and returns null for unavailable memory.
	/// This call never starts a conversation, generates memory or changes configuration.
	pub async fn memory_summary(&self, thread: &str) -> Result<Option<String>, ClientError> {
		let result =
			self.request("thread/memory/read", serde_json::json!({"threadId":thread})).await?;

		match result.get("summary") {
			Some(Value::Null) => Ok(None),
			Some(Value::String(text)) if text.len() <= MAX_MEMORY_SUMMARY_BYTES =>
				Ok((!text.trim().is_empty()).then(|| text.clone())),
			_ => Err(ClientError::InvalidFrame),
		}
	}
}
