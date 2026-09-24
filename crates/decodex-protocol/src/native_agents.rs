//! Read-only native agent observations and source-bound conversation previews.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// One provider-observed spawned agent. No local execution authority is implied.
pub struct NativeAgentDto {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Native spawning parent identity.
	pub parent_thread_id: String,
	/// Bounded display title.
	pub title: String,
	/// Provider-reported thread state.
	pub status: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// One bounded public message from the exact native conversation.
pub struct NativeAgentMessage {
	/// Exact native item identity.
	pub id: String,
	/// Public message role.
	pub role: String,
	/// Credential-filtered readable content.
	pub text: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
/// Native inspection result; missing observations are not an empty list.
pub enum NativeAgentsResult {
	/// A complete page of native descendants.
	Available {
		/// Native descendants on this page.
		agents: Vec<NativeAgentDto>,
		/// Exact cursor for the next page.
		next_cursor: Option<String>,
	},
	/// Recent conversation with explicit input capability.
	Conversation {
		/// Exact inspected thread.
		thread_id: String,
		/// Native capability; false also covers missing capability metadata.
		can_input: bool,
		/// Observed running turn for steering.
		active_turn: Option<String>,
		/// Bounded messages in chronological order.
		messages: Vec<NativeAgentMessage>,
		/// Older or oversized content was omitted.
		truncated: bool,
	},
	/// Current native state could not be verified.
	Unavailable,
}
