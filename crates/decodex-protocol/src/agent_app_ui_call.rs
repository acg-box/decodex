//! Explicit widget tool review and confirmation; browser messages are not execution authority.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{EntityId, WireText};
use decodex_core::MAX_NATIVE_MESSAGE_BYTES;

/// Bound for one reviewed local-wire invocation, including its source identity.
pub const MAX_AGENT_APP_UI_CALL_BYTES: usize = 64 * 1_024;
/// Maximum complete saved invocation and native result document.
pub const MAX_AGENT_APP_UI_RECEIPT_BYTES: usize = 2 * MAX_NATIVE_MESSAGE_BYTES + 65_536;
/// Binary chunk limit for saved tool-call readback.
pub const AGENT_APP_UI_RECEIPT_CHUNK_BYTES: usize = 32 * 1_024;

/// One browser callback identified by a fresh host-generated operation identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAppUiCall {
	/// Owning task.
	pub work_id: EntityId,
	/// Original native thread.
	pub thread_id: EntityId,
	/// Original native turn.
	pub turn_id: EntityId,
	/// Original MCP tool item.
	pub item_id: EntityId,
	/// Source identity returned with the displayed document.
	pub source_fingerprint: EntityId,
	/// Unique identity assigned by the native host, not the browser RPC id.
	pub operation_id: EntityId,
	/// Exact raw native tool name.
	pub tool: WireText,
	/// Complete arguments to show and confirm.
	pub arguments: Value,
}

/// Exact durable operation readback, independent of a live native process.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAppUiReceiptRequest {
	/// Owning task.
	pub work_id: EntityId,
	/// Host-generated operation identity used at confirmation.
	pub operation_id: EntityId,
	/// Byte offset into the complete saved document.
	pub offset: u32,
	/// Required content identity for continuation chunks.
	pub fingerprint: Option<EntityId>,
}

/// Native evidence for an explicit user confirmation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentAppUiCallReview {
	/// Current source and tool evidence. This is not permission to execute.
	Available {
		/// Exact request reviewed by the service.
		request: Box<AgentAppUiCall>,
		/// Identity of the source, descriptor and complete invocation.
		review_token: EntityId,
		/// Native server shown in the confirmation.
		server: WireText,
		/// Public tool title, falling back to the raw name.
		title: WireText,
		/// A prior unresolved call must be reviewed before another call.
		pending_operation: Option<EntityId>,
	},
	/// The complete request exceeds the supported local confirmation size.
	CapacityExceeded,
	/// The source, visibility or ownership evidence is unavailable or changed.
	Unavailable,
}

/// Durable evidence; unavailable never authorizes retry of a mutation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentAppUiReceiptResult {
	/// A bounded part of the saved invocation, status and native result.
	Available {
		/// Exact requested operation and offset.
		request: Box<AgentAppUiReceiptRequest>,
		/// Complete document hash, including saved identity and status.
		fingerprint: EntityId,
		/// Complete document length in bytes.
		total_bytes: u32,
		/// Bytes beginning at the requested offset.
		bytes: Vec<u8>,
	},
	/// No exact readable evidence, or content changed during continuation.
	Unavailable,
	/// The saved evidence exceeds the supported transfer capacity.
	CapacityExceeded,
}

/// Cold discovery from the saved work journal; no native connection is required.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentPendingAppUiCall {
	/// The work journal was read successfully.
	Available {
		/// Exact work queried.
		work_id: EntityId,
		/// Unresolved call, or none when no call needs acknowledgment.
		operation_id: Option<EntityId>,
	},
	/// The journal could not be read. This is not evidence of no pending call.
	Unavailable,
}
