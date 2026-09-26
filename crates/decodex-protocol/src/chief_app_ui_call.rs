//! Explicit widget tool review and confirmation; browser messages are not execution authority.
use serde::{Deserialize, Serialize};

/// Bound for one reviewed local-wire invocation, including its source identity.
pub const MAX_CHIEF_APP_UI_CALL_BYTES: usize = 64 * 1024;

/// One browser callback identified by a fresh host-generated operation identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefAppUiCall {
	/// Owning task.
	pub work_id: crate::EntityId,
	/// Original native thread.
	pub thread_id: crate::EntityId,
	/// Original native turn.
	pub turn_id: crate::EntityId,
	/// Original MCP tool item.
	pub item_id: crate::EntityId,
	/// Source identity returned with the displayed document.
	pub source_fingerprint: crate::EntityId,
	/// Unique identity assigned by the native host, not the browser RPC id.
	pub operation_id: crate::EntityId,
	/// Exact raw native tool name.
	pub tool: crate::WireText,
	/// Complete arguments to show and confirm.
	pub arguments: serde_json::Value,
}

/// Native evidence for an explicit user confirmation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefAppUiCallReview {
	/// Current source and tool evidence. This is not permission to execute.
	Available {
		/// Exact request reviewed by the service.
		request: Box<ChiefAppUiCall>,
		/// Identity of the source, descriptor and complete invocation.
		review_token: crate::EntityId,
		/// Native server shown in the confirmation.
		server: crate::WireText,
		/// Public tool title, falling back to the raw name.
		title: crate::WireText,
		/// A prior unresolved call must be reviewed before another call.
		pending_operation: Option<crate::EntityId>,
	},
	/// The complete request exceeds the supported local confirmation size.
	CapacityExceeded,
	/// The source, visibility or ownership evidence is unavailable or changed.
	Unavailable,
}
