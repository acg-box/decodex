use crate::{
	orchestrator::{
		self, ExternalReviewRequestCiGate, PostReviewLaneDecision,
		tests::{
			self, FakePullRequestReviewStateInspector,
			review_landing_classification_checks::landing_gates::support::{
				self, BRANCH_NAME, HEAD_OID, PR_URL,
			},
		},
	},
	pull_request::PullRequestRequiredStatusContext,
};

#[test]
fn classify_post_review_lane_waits_for_pending_required_checks_before_ready_to_land() {
	let (_temp_dir, state_store, snapshot) =
		support::snapshot_for_issue_state("In Review", BRANCH_NAME);

	support::seed_review_marker(&state_store, &snapshot, "pass_waiting_for_gates", 1);

	let mut review_state = tests::sample_pull_request_review_state(
		PR_URL,
		BRANCH_NAME,
		HEAD_OID,
		Some("APPROVED"),
		"MERGEABLE",
		"UNSTABLE",
		Some("PENDING"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = support::classify(
		&snapshot,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	);

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_passed_waiting_gates");
}

#[test]
fn review_state_gates_allow_configured_status_context_when_rollup_pending() {
	let mut review_state = tests::sample_pull_request_review_state(
		PR_URL,
		BRANCH_NAME,
		HEAD_OID,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("PENDING"),
		0,
	);
	review_state.required_status_contexts = vec![PullRequestRequiredStatusContext {
		context: String::from("decodex/local-full-check"),
		state: Some(String::from("success")),
		creator_login: Some(String::from("decodex-bot")),
		allowed_creator: true,
		base_ref_oid: Some(String::from("base-sha")),
		base_ref_matches: true,
	}];

	assert_eq!(
		orchestrator::external_review_request_ci_gate(&review_state),
		ExternalReviewRequestCiGate::Ready
	);
	assert!(orchestrator::review_state_clean_path_landing_gates_satisfied(&review_state));
}

#[test]
fn review_state_gates_wait_for_configured_status_context_on_stale_base() {
	let mut review_state = tests::sample_pull_request_review_state(
		PR_URL,
		BRANCH_NAME,
		HEAD_OID,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	review_state.required_status_contexts = vec![PullRequestRequiredStatusContext {
		context: String::from("decodex/local-full-check"),
		state: Some(String::from("success")),
		creator_login: Some(String::from("decodex-bot")),
		allowed_creator: true,
		base_ref_oid: Some(String::from("old-base-sha")),
		base_ref_matches: false,
	}];

	assert_eq!(
		orchestrator::external_review_request_ci_gate(&review_state),
		ExternalReviewRequestCiGate::WaitForGreenChecks
	);
	assert!(!orchestrator::review_state_clean_path_landing_gates_satisfied(&review_state));
}

#[test]
fn configured_status_context_success_ignores_failed_global_rollup() {
	let mut review_state = tests::sample_pull_request_review_state(
		PR_URL,
		BRANCH_NAME,
		HEAD_OID,
		Some("APPROVED"),
		"MERGEABLE",
		"BLOCKED",
		Some("FAILURE"),
		0,
	);
	review_state.required_status_contexts = vec![PullRequestRequiredStatusContext {
		context: String::from("decodex/local-full-check"),
		state: Some(String::from("success")),
		creator_login: Some(String::from("decodex-bot")),
		allowed_creator: true,
		base_ref_oid: Some(String::from("base-sha")),
		base_ref_matches: true,
	}];

	assert!(!orchestrator::review_state_checks_require_repair(&review_state));
}

#[test]
fn classify_post_review_lane_routes_non_clean_landing_to_agent_fallback() {
	for (merge_state, status_check_state) in
		[("HAS_HOOKS", Some("SUCCESS")), ("UNSTABLE", Some("FAILURE"))]
	{
		let (_temp_dir, state_store, snapshot) =
			support::snapshot_for_issue_state("In Review", BRANCH_NAME);

		support::seed_review_marker(&state_store, &snapshot, "waiting_for_result", 1);

		let mut review_state = tests::sample_pull_request_review_state(
			PR_URL,
			BRANCH_NAME,
			HEAD_OID,
			Some("APPROVED"),
			"MERGEABLE",
			merge_state,
			status_check_state,
			0,
		);

		tests::add_external_review_ack(&mut review_state);
		tests::add_external_review_pass(&mut review_state);

		let classification = support::classify(
			&snapshot,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		);

		assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
		assert_eq!(classification.reason, "retained_landing_agent_fallback_required");
	}
}

#[test]
fn classify_post_review_lane_waits_for_review_before_optional_failed_checks() {
	let (_temp_dir, state_store, snapshot) =
		support::snapshot_for_issue_state("In Review", BRANCH_NAME);

	support::seed_review_marker(&state_store, &snapshot, "waiting_for_result", 0);

	let mut review_state = tests::sample_pull_request_review_state(
		PR_URL,
		BRANCH_NAME,
		HEAD_OID,
		Some("REVIEW_REQUIRED"),
		"MERGEABLE",
		"UNSTABLE",
		Some("FAILURE"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);

	let classification = support::classify(
		&snapshot,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	);

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_result_pending");
}

#[test]
fn requires_repair_before_review_when_checks_fail() {
	let (_temp_dir, state_store, snapshot) =
		support::snapshot_for_issue_state("In Review", BRANCH_NAME);
	let classification = support::classify(
		&snapshot,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				support::PR_URL,
				support::BRANCH_NAME,
				support::HEAD_OID,
				Some("REVIEW_REQUIRED"),
				"MERGEABLE",
				"BLOCKED",
				Some("FAILURE"),
				0,
			),
		)]),
	);

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "required_checks_failed");
}
