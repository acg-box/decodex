//! Native read receipts are separate from local handoff and attention state.
use crate::{EntityId, WireText};
use serde::{Deserialize, Serialize};
/// First unread native result.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentUnreadPosition {
	/// The user explicitly marked this conversation unread.
	ThreadStart,
	/// The earliest unseen completed turn.
	Turn {
		/// Native turn identity.
		#[serde(rename = "turnId")]
		turn_id: String,
	},
}
/// A native observation bound to the exact local owner and process.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentReadStateResult {
	/// The native source returned an eligible durable receipt.
	Available {
		/// Local task owner.
		work_id: EntityId,
		/// Native conversation identity.
		thread_id: EntityId,
		/// Null means read. This is never inferred from unavailable metadata.
		first_unread: Option<AgentUnreadPosition>,
		/// Native opaque revision, used only for equality within this thread.
		revision: WireText,
		/// Local source identity bound to this native revision.
		review_token: WireText,
	},
	/// Unsupported, ineligible, malformed, or no longer current.
	Unavailable,
}
