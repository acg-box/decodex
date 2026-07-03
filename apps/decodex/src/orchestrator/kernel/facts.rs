use crate::orchestrator::kernel::state::{LivenessState, TerminalizationState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LaneObservation {
	pub(crate) issue_id: String,
	pub(crate) run_id: Option<String>,
	pub(crate) run_lease: bool,
	pub(crate) authority_complete: bool,
	pub(crate) liveness: LivenessState,
	pub(crate) terminalization: TerminalizationState,
	pub(crate) active_owned_work: bool,
	pub(crate) external_signal_pending: bool,
	pub(crate) retry_budget_available: bool,
	pub(crate) retry_budget_exhausted: bool,
	pub(crate) retained_lane_reusable: bool,
	pub(crate) ready_to_land: bool,
	pub(crate) human_attention_signal: bool,
	pub(crate) contradictory_authority: bool,
	pub(crate) post_review_lifecycle_required: bool,
	pub(crate) post_review_lifecycle_present: bool,
}
impl LaneObservation {
	pub(crate) fn for_issue(issue_id: impl Into<String>) -> Self {
		Self {
			issue_id: issue_id.into(),
			run_id: None,
			run_lease: false,
			authority_complete: false,
			liveness: LivenessState::Unknown,
			terminalization: TerminalizationState::None,
			active_owned_work: false,
			external_signal_pending: false,
			retry_budget_available: false,
			retry_budget_exhausted: false,
			retained_lane_reusable: false,
			ready_to_land: false,
			human_attention_signal: false,
			contradictory_authority: false,
			post_review_lifecycle_required: false,
			post_review_lifecycle_present: false,
		}
	}
}
