//! Atomic refusal history for a reserved turn that never reached model dispatch.
use decodex_core::{ConversationId, HistoryItemId, RuntimeSessionId, TurnId};
use rusqlite::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{incompatible, read_receipt, sql_error, touch_conversation, write_receipt};
use crate::{CommandIdentity, SqliteStore, StoreError, unix_micros};

/// Closed, non-sensitive cause established by the native resume response.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationResumeRejection {
	/// The exact thread is still closing after bounded resume retries.
	ClosingThread,
	/// Native storage explicitly reports the exact requested thread missing.
	MissingThread,
	/// The exact requested thread exists in the native archive.
	ArchivedThread,
	/// Native filesystem sandbox preparation failed.
	SandboxConfiguration,
	/// Native admission stopped on the current connection.
	ServerDraining,
	/// Managed provider requirements require a fresh connection.
	ManagedProviderChanged,
	/// Native resume rejected the request without a more specific safe classification.
	Other,
}

impl ConversationResumeRejection {
	/// Stable diagnostic text, without provider error text or credentials.
	pub const fn diagnostic(self) -> &'static str {
		match self {
			Self::ClosingThread =>
				"Codex is still closing this thread. This input was not sent. Wait briefly, then refresh this conversation.",
			Self::MissingThread =>
				"Codex could not find this thread. This input was not sent. Check the selected account and native thread storage before starting another conversation.",
			Self::ArchivedThread =>
				"Codex refused to resume an archived thread. This input was not sent. Unarchive the existing Codex thread, then refresh this conversation.",
			Self::SandboxConfiguration =>
				"Codex could not prepare its filesystem sandbox. This input was not sent. Check sandbox permissions and writable roots, then refresh this conversation.",
			Self::ServerDraining =>
				"Codex is draining this connection. This input was not sent. Reconnect before sending again.",
			Self::ManagedProviderChanged =>
				"Codex provider requirements changed. This input was not sent. Restart Codex before sending again.",
			Self::Other =>
				"Codex rejected this thread resume. This input was not sent. Check the model, provider and project configuration, then refresh this conversation.",
		}
	}
}

/// Exact local authority and response witness for one rejected native resume.
#[derive(Clone, Debug, Serialize)]
pub struct RecordConversationResumeRejection {
	/// Conversation that owns the reserved turn.
	pub conversation_id: ConversationId,
	/// Session whose native identity was resumed.
	pub runtime_session_id: RuntimeSessionId,
	/// Session revision observed before the request.
	pub expected_session_revision: i64,
	/// Exact native thread identity.
	pub thread_id: String,
	/// Reserved user turn, still at revision one and without a provider attempt.
	pub turn_id: TurnId,
	/// Stable identifier for the diagnostic history item.
	pub history_item_id: HistoryItemId,
	/// Classified native refusal.
	pub reason: ConversationResumeRejection,
	/// SHA-256 of the exact rejection response; raw provider text is not stored.
	pub witness_digest: String,
}

impl SqliteStore {
	/// Atomically record the refusal and fail the unsent reserved turn. Replays are idempotent.
	pub async fn record_conversation_resume_rejection(
		&self,
		command: &CommandIdentity,
		request: &RecordConversationResumeRejection,
	) -> Result<(), StoreError> {
		if request.expected_session_revision <= 0
			|| request.thread_id.is_empty()
			|| request.witness_digest.len() != 64
			|| !request.witness_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
		{
			return Err(StoreError::InvalidInput("invalid resume rejection coordinates"));
		}
		let command = command.clone();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			if let Some(receipt) = read_receipt(&transaction, &command, "reject_conversation_resume", request.turn_id.as_str())? {
				if receipt != request.history_item_id.as_str() {
					return Err(incompatible("resume rejection receipt"));
				}
				transaction.commit().map_err(sql_error)?;
				return Ok(());
			}
			let now = unix_micros().map_err(StoreError::from)?;
			let changed = transaction.execute(
				"UPDATE turns SET status = 'failed', revision = 2, updated_at_micros = ?6, completed_at_micros = ?6
				 WHERE turn_id = ?1 AND conversation_id = ?2 AND runtime_session_id = ?3
				 AND role = 'user' AND status = 'active' AND revision = 1
				 AND EXISTS (SELECT 1 FROM runtime_sessions s JOIN conversations c USING (conversation_id)
				   WHERE s.runtime_session_id = ?3 AND s.conversation_id = ?2 AND s.revision = ?4
				   AND s.codex_thread_id = ?5 AND s.state = 'active' AND c.state = 'active' AND c.kind = 'ordinary_task')
				 AND NOT EXISTS (SELECT 1 FROM provider_attempts WHERE turn_id = ?1)
				 AND NOT EXISTS (SELECT 1 FROM history_items WHERE turn_id = ?1 AND status = 'streaming')",
				params![request.turn_id.as_str(), request.conversation_id.as_str(), request.runtime_session_id.as_str(), request.expected_session_revision, request.thread_id, now],
			).map_err(sql_error)?;
			if changed != 1 {
				return Err(incompatible("resume rejection no longer owns an unsent turn"));
			}
			let metadata = serde_json::json!({"type":"native_resume_rejection", "reason":request.reason, "response_sha256":request.witness_digest}).to_string();
			transaction.execute(
				"INSERT INTO history_items (history_item_id, conversation_id, turn_id, sequence, kind, role, status,
				 media_type, inline_text, metadata_json, revision, created_at_micros, updated_at_micros)
				 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM history_items WHERE conversation_id = ?2),
				 'status', 'user', 'failed', 'text/plain', ?4, ?5, 1, ?6, ?6)",
				params![request.history_item_id.as_str(), request.conversation_id.as_str(), request.turn_id.as_str(), request.reason.diagnostic(), metadata, now],
			).map_err(sql_error)?;
			touch_conversation(&transaction, &request.conversation_id, now)?;
			write_receipt(&transaction, &command, "reject_conversation_resume", request.turn_id.as_str(), request.history_item_id.as_str(), now)?;
			transaction.commit().map_err(sql_error)?;
			Ok(())
		}).await
	}
}
