mod gates;
mod model;

pub(crate) use self::{
	gates::{
		classify_landing_gate, failed_checks_require_repair, manual_landing_gates_satisfied,
		merge_state_requires_review_repair, mergeability_unknown,
		retained_clean_path_landing_gates_satisfied, retained_landing_gates_satisfied,
		retained_landing_requires_agent_fallback,
	},
	model::{
		LandingGateDecision, LandingGateMode, PullRequestLandingGateView, PullRequestLandingState,
		PullRequestRequiredStatusContext,
	},
};

#[cfg(test)] mod tests;
