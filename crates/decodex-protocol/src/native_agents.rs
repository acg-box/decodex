//! Read-only native agent observations and source-bound input capability.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// One provider-observed spawned agent. No local execution authority is implied.
pub struct NativeAgentDto {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Native spawning parent identity.
	pub parent_thread_id: String,
	/// Bounded display title.
	pub title: String,
	/// Bounded first native input, used to identify the delegated work.
	#[serde(default)]
	pub task: String,
	/// Provider-reported thread state.
	pub status: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
	/// Conversation identity with explicit input capability.
	Conversation {
		/// Exact inspected thread.
		thread_id: String,
		/// Native capability. None means the stored thread has not exposed its capability.
		can_input: Option<bool>,
		/// Observed running turn for steering.
		active_turn: Option<String>,
	},
	/// Current native state could not be verified.
	Unavailable,
}
