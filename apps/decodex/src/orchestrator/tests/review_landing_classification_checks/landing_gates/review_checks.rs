use crate::orchestrator::{
	PostReviewLaneDecision,
	tests::{
		self, FakePullRequestReviewStateInspector,
		review_landing_classification_checks::landing_gates::support::{
			self, BRANCH_NAME, HEAD_OID, PR_URL,
		},
	},
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
fn classify_post_review_lane_requires_review_repair_before_review_when_required_checks_fail() {
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
