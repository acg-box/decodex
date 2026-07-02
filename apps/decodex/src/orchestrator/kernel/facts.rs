use super::state::{LivenessState, TerminalizationState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) struct LaneObservation {
	pub(in crate::orchestrator) issue_id: String,
	pub(in crate::orchestrator) run_id: Option<String>,
	pub(in crate::orchestrator) run_lease: bool,
	pub(in crate::orchestrator) authority_complete: bool,
	pub(in crate::orchestrator) liveness: LivenessState,
	pub(in crate::orchestrator) terminalization: TerminalizationState,
	pub(in crate::orchestrator) active_owned_work: bool,
	pub(in crate::orchestrator) external_signal_pending: bool,
	pub(in crate::orchestrator) retry_budget_available: bool,
	pub(in crate::orchestrator) retry_budget_exhausted: bool,
	pub(in crate::orchestrator) retained_lane_reusable: bool,
	pub(in crate::orchestrator) ready_to_land: bool,
	pub(in crate::orchestrator) human_attention_signal: bool,
	pub(in crate::orchestrator) contradictory_authority: bool,
	pub(in crate::orchestrator) post_review_lifecycle_required: bool,
	pub(in crate::orchestrator) post_review_lifecycle_present: bool,
}

impl LaneObservation {
	pub(in crate::orchestrator) fn for_issue(issue_id: impl Into<String>) -> Self {
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
