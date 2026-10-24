//! Global native review preference and truthful application receipts.
use serde::{Deserialize, Serialize};

/// Global review routing; execution permissions remain native and separate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPreference {
	/// Eligible approvals require the user's answer.
	#[default]
	AskMe,
	/// Eligible approvals go through native Guardian.
	AutomaticReview,
}
impl ReviewPreference {
	/// Native protocol value, not a permission profile.
	pub const fn native(self) -> &'static str {
		match self {
			Self::AskMe => "user",
			Self::AutomaticReview => "auto_review",
		}
	}
}
/// Saved global intent. Saving does not establish native application.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct GuardianSettings {
	/// Monotonic local revision used to reject stale settings edits.
	pub revision: u64,
	/// Desired review routing across native sources.
	pub reviewer: ReviewPreference,
	/// Opt in to supported private review observation.
	pub review_details: bool,
}
impl Default for GuardianSettings {
	fn default() -> Self {
		Self { revision: 1, reviewer: ReviewPreference::AskMe, review_details: false }
	}
}
/// One exact source's application receipt. This is an observation, never an override control.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct GuardianSource {
	/// Exact retained process generation, or transport identity in an isolated adapter.
	pub source_id: String,
	/// Persistent Agent using the native source.
	pub agent_id: String,
	/// Exact native conversation.
	pub thread_id: String,
	/// Global intent revision being applied.
	pub revision: u64,
	/// Pending, applied, overridden, unsupported, rejected, or unknown.
	pub outcome: String,
	/// Actual readback, absent if unavailable.
	pub effective_reviewer: Option<String>,
	/// Setting or qualification limit.
	pub detail: String,
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
	/// Actual native application receipts, not per-Agent controls.
	pub sources: Vec<GuardianSource>,
	/// Bounded optional observer records.
	pub details: Vec<ReviewDetails>,
	/// Subscription and evidence limits, including unassociated records and reconnect gaps.
	pub observation_limit: String,
}
