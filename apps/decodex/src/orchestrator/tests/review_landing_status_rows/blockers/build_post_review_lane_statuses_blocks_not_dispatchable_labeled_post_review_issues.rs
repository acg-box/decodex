use crate::orchestrator::{
	self, StateStore,
	tests::{self, FakePullRequestReviewStateInspector, FakeTracker},
};

#[test]
fn build_post_review_lane_statuses_blocks_not_dispatchable_labeled_post_review_issues() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	for (labels, expected_reason) in [
		(&["decodex:manual-only"][..], "issue_opted_out"),
		(&["decodex:needs-attention"][..], "issue_needs_attention"),
	] {
		let issue = tests::sample_issue("In Review", labels);

		state_store
			.upsert_worktree(
				config.service_id(),
				&issue.id,
				"x/pubfi-pub-101",
				&config.repo_root().display().to_string(),
			)
			.expect("worktree should record");

		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let lanes = orchestrator::build_post_review_lane_statuses(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(Vec::new()),
		)
		.expect("post-review lane status build should succeed");

		assert_eq!(lanes.len(), 1);
		assert_eq!(lanes[0].classification, "blocked");
		assert_eq!(lanes[0].reason, expected_reason);

		state_store.clear_worktree(&issue.id).expect("worktree should clear between label cases");
	}
}
