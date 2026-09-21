//! Current-turn reviewer controls and local publication receipts.
use serde::{Deserialize, Serialize};

/// A local attempt result, never a claim about effective tool policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChiefLiveReviewerOutcome {
	/// Reserved before dispatch; delivery may be unresolved.
	Reserved,
	/// Native published the requested reviewer for subsequent captures.
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
pub enum ChiefLiveReviewerState {
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
		last_reviewer: Option<crate::ChiefAppReviewer>,
		/// Last local publication receipt. None means no recorded local edit.
		last_outcome: Option<ChiefLiveReviewerOutcome>,
	},
	/// No exact editable live task is available.
	Unavailable,
}
