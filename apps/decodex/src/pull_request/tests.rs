use crate::pull_request::{
	self, PullRequestLandingGateView, PullRequestRequiredStatusContext, gates,
};

fn sample_gate_view() -> PullRequestLandingGateView<'static> {
	PullRequestLandingGateView {
		state: "OPEN",
		is_draft: false,
		review_decision: Some("APPROVED"),
		pending_review_requests: 0,
		mergeable: "MERGEABLE",
		merge_state_status: "CLEAN",
		status_check_rollup_state: Some("SUCCESS"),
		required_status_contexts: &[],
		unresolved_review_threads: 0,
	}
}

fn successful_decodex_status_context() -> Vec<PullRequestRequiredStatusContext> {
	vec![PullRequestRequiredStatusContext {
		context: String::from("decodex/local-full-check"),
		state: Some(String::from("success")),
		creator_login: Some(String::from("decodex-bot")),
		allowed_creator: true,
		base_ref_oid: Some(String::from("base-sha")),
		base_ref_matches: true,
	}]
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
fn configured_required_status_contexts_replace_global_rollup_for_landing() {
	let contexts = successful_decodex_status_context();
	let mut view = sample_gate_view();

	view.status_check_rollup_state = Some("PENDING");
	view.required_status_contexts = &contexts;

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Satisfied
	);
	assert!(pull_request::retained_clean_path_landing_gates_satisfied(view));
}

#[test]
fn configured_required_status_contexts_fail_closed_by_state_and_creator() {
	let mut contexts = successful_decodex_status_context();
	let mut view = sample_gate_view();

	contexts[0].state = Some(String::from("pending"));
	view.required_status_contexts = &contexts;

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Wait("required_status_context_waiting")
	);

	let mut contexts = successful_decodex_status_context();
	let mut view = sample_gate_view();

	contexts[0].state = Some(String::from("failure"));
	view.required_status_contexts = &contexts;

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Repair("required_status_context_failed")
	);

	let mut contexts = successful_decodex_status_context();
	let mut view = sample_gate_view();

	contexts[0].state = Some(String::from("success"));
	contexts[0].allowed_creator = false;
	view.required_status_contexts = &contexts;

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Block("required_status_context_creator_mismatch")
	);

	let mut contexts = successful_decodex_status_context();
	let mut view = sample_gate_view();

	contexts[0].base_ref_matches = false;
	view.required_status_contexts = &contexts;

	assert_eq!(
		pull_request::classify_landing_gate(view, pull_request::LandingGateMode::ManualLand),
		pull_request::LandingGateDecision::Wait("required_status_context_base_stale")
	);
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
