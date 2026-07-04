use crate::orchestrator::{
	PostReviewLaneDecision,
	tests::{
		self, FakePullRequestReviewStateInspector,
		review_landing_classification_checks::landing_gates::support::{self, BRANCH_NAME},
	},
};

#[test]
fn classify_post_review_lane_blocks_completed_issue_until_pull_request_is_merged() {
	let (_temp_dir, state_store, snapshot) = support::snapshot_for_issue_state("Done", BRANCH_NAME);
	let classification = support::classify(
		&snapshot,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				support::PR_URL,
				support::BRANCH_NAME,
				support::HEAD_OID,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	);

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "issue_completed_before_pull_request_merged");
}
