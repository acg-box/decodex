use crate::{
	orchestrator::status::{
		ExternalReviewRequestCiGate, PullRequestLandingGateView, PullRequestReviewState,
	},
	pull_request,
};

pub(crate) fn external_review_request_ci_gate(
	review_state: &PullRequestReviewState,
) -> ExternalReviewRequestCiGate {
	match review_state.status_check_rollup_state.as_deref() {
		None | Some("SUCCESS") => ExternalReviewRequestCiGate::Ready,
		Some("EXPECTED" | "PENDING") => ExternalReviewRequestCiGate::WaitForGreenChecks,
		Some("ERROR" | "FAILURE") => ExternalReviewRequestCiGate::RepairRequired,
		Some(_) => ExternalReviewRequestCiGate::WaitForGreenChecks,
	}
}

pub(crate) fn failed_checks_require_repair(
	check_state: Option<&str>,
	merge_state_status: &str,
) -> bool {
	pull_request::failed_checks_require_repair(check_state, merge_state_status)
}

pub(crate) fn merge_state_requires_review_repair(
	mergeable: &str,
	merge_state_status: &str,
) -> Option<&'static str> {
	pull_request::merge_state_requires_review_repair(mergeable, merge_state_status)
}

pub(crate) fn review_state_landing_gates_satisfied(review_state: &PullRequestReviewState) -> bool {
	pull_request::retained_landing_gates_satisfied(review_state_landing_gate_view(review_state))
}

pub(crate) fn review_state_clean_path_landing_gates_satisfied(
	review_state: &PullRequestReviewState,
) -> bool {
	pull_request::retained_clean_path_landing_gates_satisfied(review_state_landing_gate_view(
		review_state,
	))
}

pub(crate) fn review_state_landing_requires_agent_fallback(
	review_state: &PullRequestReviewState,
) -> bool {
	pull_request::retained_landing_requires_agent_fallback(review_state_landing_gate_view(
		review_state,
	))
}

fn review_state_landing_gate_view(
	review_state: &PullRequestReviewState,
) -> PullRequestLandingGateView<'_> {
	PullRequestLandingGateView {
		state: review_state.state.as_str(),
		is_draft: review_state.is_draft,
		review_decision: review_state.review_decision.as_deref(),
		pending_review_requests: review_state.pending_review_requests,
		mergeable: review_state.mergeable.as_str(),
		merge_state_status: review_state.merge_state_status.as_str(),
		status_check_rollup_state: review_state.status_check_rollup_state.as_deref(),
		unresolved_review_threads: review_state.unresolved_review_threads,
	}
}
