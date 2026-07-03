use crate::pull_request::{self, PullRequestLandingGateView, gates};

fn sample_gate_view() -> PullRequestLandingGateView<'static> {
	PullRequestLandingGateView {
		state: "OPEN",
		is_draft: false,
		review_decision: Some("APPROVED"),
		pending_review_requests: 0,
		mergeable: "MERGEABLE",
		merge_state_status: "CLEAN",
		status_check_rollup_state: Some("SUCCESS"),
		unresolved_review_threads: 0,
	}
}

#[test]
fn landing_gates_handle_green_pending_and_review_request_cases() {
	assert!(pull_request::manual_landing_gates_satisfied(sample_gate_view()));

	let mut view = sample_gate_view();

	view.status_check_rollup_state = Some("PENDING");

	assert!(!pull_request::manual_landing_gates_satisfied(view));
	assert!(gates::checks_require_wait(Some("PENDING")));

	let mut view = sample_gate_view();

	view.pending_review_requests = 2;

	assert!(pull_request::retained_landing_gates_satisfied(view));
	assert!(!pull_request::manual_landing_gates_satisfied(view));
}

#[test]
fn landing_gate_classifier_keeps_raw_github_states_but_types_decisions() {
	assert_eq!(
		pull_request::classify_landing_gate(
			sample_gate_view(),
			pull_request::LandingGateMode::ManualLand,
		),
		pull_request::LandingGateDecision::Satisfied
	);

	let mut view = sample_gate_view();

	view.state = "MERGED";

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::CloseoutOnly
	);
	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::Adopt),
		pull_request::LandingGateDecision::Block("pull_request_not_open")
	);

	let mut view = sample_gate_view();

	view.merge_state_status = "BLOCKED";

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Block("merge_state_not_ready")
	);

	let mut view = sample_gate_view();

	view.status_check_rollup_state = Some("PENDING");

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Wait("checks_waiting")
	);
}

#[test]
fn clean_path_landing_gates_only_allow_current_green_prs() {
	assert!(pull_request::retained_clean_path_landing_gates_satisfied(sample_gate_view()));

	let mut view = sample_gate_view();

	view.merge_state_status = "HAS_HOOKS";

	assert!(pull_request::retained_landing_gates_satisfied(view));
	assert!(pull_request::retained_landing_requires_agent_fallback(view));
	assert!(!pull_request::retained_clean_path_landing_gates_satisfied(view));

	let mut view = sample_gate_view();

	view.merge_state_status = "UNSTABLE";
	view.status_check_rollup_state = Some("FAILURE");

	assert!(pull_request::retained_landing_gates_satisfied(view));
	assert!(pull_request::retained_landing_requires_agent_fallback(view));
	assert!(!pull_request::retained_clean_path_landing_gates_satisfied(view));

	let mut view = sample_gate_view();

	view.mergeable = "UNKNOWN";

	assert!(!pull_request::retained_landing_gates_satisfied(view));
	assert!(pull_request::retained_landing_requires_agent_fallback(view));

	let mut view = sample_gate_view();

	view.merge_state_status = "UNKNOWN";

	assert!(!pull_request::retained_landing_gates_satisfied(view));
	assert!(pull_request::retained_landing_requires_agent_fallback(view));
}

#[test]
fn landing_gates_route_conflicts_and_branch_lag_to_repair() {
	assert_eq!(
		pull_request::merge_state_requires_review_repair("CONFLICTING", "CLEAN"),
		Some("pull_request_merge_conflict")
	);
	assert_eq!(
		pull_request::merge_state_requires_review_repair("MERGEABLE", "BEHIND"),
		Some("pull_request_branch_behind_base")
	);
}

#[test]
fn merge_state_allows_ready_to_land_matches_existing_runtime_policy() {
	assert!(gates::merge_state_allows_ready_to_land("CLEAN"));
	assert!(gates::merge_state_allows_ready_to_land("HAS_HOOKS"));
	assert!(gates::merge_state_allows_ready_to_land("UNSTABLE"));
	assert!(!gates::merge_state_allows_ready_to_land("BLOCKED"));
}

#[test]
fn failed_checks_require_repair_only_for_blocked_red_checks() {
	assert!(pull_request::failed_checks_require_repair(Some("FAILURE"), "BLOCKED"));
	assert!(!pull_request::failed_checks_require_repair(Some("FAILURE"), "CLEAN"));
}

#[test]
fn landing_gate_helpers_detect_unknown_mergeability() {
	let mut view = sample_gate_view();

	view.mergeable = "UNKNOWN";

	assert!(pull_request::mergeability_unknown(view));

	let mut view = sample_gate_view();

	view.merge_state_status = "UNKNOWN";

	assert!(pull_request::mergeability_unknown(view));
	assert!(!pull_request::mergeability_unknown(sample_gate_view()));
}
