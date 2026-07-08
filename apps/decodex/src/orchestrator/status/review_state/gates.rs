use crate::{
	orchestrator::status::{
		ExternalReviewRequestCiGate, PullRequestLandingGateView, PullRequestReviewState,
	},
	pull_request,
};

pub(crate) fn external_review_request_ci_gate(
	review_state: &PullRequestReviewState,
) -> ExternalReviewRequestCiGate {
	if !review_state.required_status_contexts.is_empty() {
		return match pull_request::classify_landing_gate(
			review_state_landing_gate_view(review_state),
			pull_request::LandingGateMode::Retained,
		) {
			pull_request::LandingGateDecision::Repair(
				"required_status_context_failed" | "required_checks_failed",
			) => ExternalReviewRequestCiGate::RepairRequired,
			pull_request::LandingGateDecision::Wait(
				"required_status_context_missing"
				| "required_status_context_waiting"
				| "required_status_context_base_stale",
			)
			| pull_request::LandingGateDecision::Block(
				"required_status_context_creator_mismatch",
			) => ExternalReviewRequestCiGate::WaitForGreenChecks,
			_ => ExternalReviewRequestCiGate::Ready,
		};
	}

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

pub(crate) fn review_state_checks_require_repair(review_state: &PullRequestReviewState) -> bool {
	if !review_state.required_status_contexts.is_empty() {
		return matches!(
			pull_request::classify_landing_gate(
				review_state_landing_gate_view(review_state),
				pull_request::LandingGateMode::Retained,
			),
			pull_request::LandingGateDecision::Repair(_)
		);
	}

	failed_checks_require_repair(
		review_state.status_check_rollup_state.as_deref(),
		&review_state.merge_state_status,
	)
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
		required_status_contexts: &review_state.required_status_contexts,
		unresolved_review_threads: review_state.unresolved_review_threads,
	}
}
