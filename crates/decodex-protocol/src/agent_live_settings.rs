//! Current-turn settings controls and local publication receipts.
use serde::{Deserialize, Serialize};

/// Native approval reviewer selection shared by reviewer controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentReviewer {
	/// Send approval requests to the user.
	User,
	/// Use native automatic review.
	AutoReview,
}

/// A local attempt result, never a claim about effective tool policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLiveReviewerOutcome {
	/// Reserved before dispatch; delivery may be unresolved.
	Reserved,
	/// Native published the requested settings for subsequent captures.
	Applied,
	/// The exact native task was no longer available.
	TargetUnavailable,
	/// The edit was rejected.
	Rejected,
	/// Publication could not be confirmed; no automatic retry is allowed.
	Unknown,
}

/// Exact task inspected for an explicit edit, with the last local attempt if present.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentLiveReviewerState {
	/// The task remains bound to the same process and account source.
	Available {
		/// Exact native thread; never follows a replacement.
		thread_id: crate::EntityId,
		/// Exact active turn; never follows a successor.
		turn_id: crate::EntityId,
		/// Opaque source and receipt version to review before editing.
		review_token: crate::WireText,
		/// False while a previous operation on this process is still unresolved.
		can_update: bool,
		/// The last requested reviewer, not necessarily the current effective reviewer.
		last_reviewer: Option<crate::AgentReviewer>,
		/// Last locally requested model and effort, not observed inference settings.
		last_model: Option<crate::AgentLiveModelSelection>,
		/// Account-bound choices when explicitly requested and live switching is enabled.
		model_choices: Option<Vec<crate::AgentModelDto>>,
		/// Last local publication receipt. None means no recorded local edit.
		last_outcome: Option<AgentLiveReviewerOutcome>,
	},
	/// No exact editable live task is available.
	Unavailable,
}

/// One explicit model/effort selection recorded for a running turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLiveModelSelection {
	/// Requested model identifier.
	pub model: crate::ConversationModel,
	/// Requested reasoning effort.
	pub effort: crate::ConversationReasoningEffort,
}
