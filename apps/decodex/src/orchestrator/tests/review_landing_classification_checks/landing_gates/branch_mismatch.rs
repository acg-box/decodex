use crate::orchestrator::{
	PostReviewLaneDecision,
	tests::{
		FakePullRequestReviewStateInspector,
		review_landing_classification_checks::landing_gates::support,
	},
};

#[test]
fn classify_post_review_lane_blocks_checkout_branch_mismatch() {
	let (_temp_dir, state_store, snapshot) =
		support::snapshot_for_issue_state("In Review", "x/pubfi-pub-999");
	let classification = support::classify(
		&snapshot,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	);

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "worktree_checkout_branch_mismatch");
}
