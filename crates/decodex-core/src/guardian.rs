//! Optional native review observations. Execution settings belong to each conversation.
use serde::{Deserialize, Serialize};

/// Global observation preferences; these never change execution permissions.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct GuardianSettings {
	/// Monotonic local revision used to reject stale settings edits.
	pub revision: u64,
	/// Opt in to supported private review observation.
	pub review_details: bool,
}
impl Default for GuardianSettings {
	fn default() -> Self {
		Self { revision: 1, review_details: false }
	}
}
/// Supported native private record. Missing association remains explicit.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ReviewDetails {
	/// Verified native parent, independent of a missing action-level association.
	pub parent_thread_id: Option<String>,
	/// The displayed record is shorter than the retained native observation.
	pub truncated: bool,
	/// Native private reviewer conversation, distinct from the parent.
	pub reviewer_thread_id: String,
	/// Exact private turn.
	pub reviewer_turn_id: String,
	/// Exact recorded item or interruption identity.
	pub item_id: String,
	/// Exact Review association, absent if the protocol does not supply it.
	pub review_id: Option<String>,
	/// Bounded supported public record, never a promise of complete internal reasoning.
	pub record: String,
}
/// Global controls and per-source observations, read through the ordinary application snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct GuardianState {
	/// Saved preference.
	pub settings: GuardianSettings,
	/// Bounded optional observer records.
	pub details: Vec<ReviewDetails>,
	/// Subscription and evidence limits, including unassociated records and reconnect gaps.
	pub observation_limit: String,
}
