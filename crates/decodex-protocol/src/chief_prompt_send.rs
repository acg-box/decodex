//! Exact canonical send identity. Missing readback never authorizes replay.
use crate::{ChiefExecutionOverrides, EntityId, IdempotencyKey, Sha256Digest, WireText};
use serde::{Deserialize, Serialize};

/// Retain with the unchanged canonical draft before submitting it once.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInputSend {
	/// Immutable staged content record.
	pub input_id: i64,
	/// Complete canonical content digest.
	pub sha256: Sha256Digest,
	/// Original logical command identity. Never generate a replacement after uncertainty.
	pub command_key: IdempotencyKey,
	/// Execution choices captured for this send.
	pub execution: ChiefExecutionOverrides,
}

/// Full owner binding for read-only acceptance reconciliation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInputSendIdentity {
	/// Original local owner.
	pub work_id: EntityId,
	/// Original native thread.
	pub thread_id: WireText,
	/// Applied and acknowledged edit receipt.
	pub edit_receipt_id: i64,
	/// Exact immutable input and command.
	pub send: PromptInputSend,
}

/// Queue acceptance is distinct from native execution or completion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptInputSendStatus {
	/// Exact requested binding, echoed without modification.
	pub identity: PromptInputSendIdentity,
	/// Existing durable user-message event, if exact acceptance can be proven.
	/// None means unknown, including when no event is visible yet. Never replay on None.
	pub accepted_event_id: Option<i64>,
}
