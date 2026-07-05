use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn live_operator_status_snapshot_degrades_when_post_review_status_refresh_fails() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue("In Review", &[]);
	let tracker = FakeTracker::with_refresh_error(vec![issue.clone()], "rate limited");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should degrade instead of failing");

	assert_eq!(snapshot.warnings, vec![String::from("post_review_lane_status_unavailable")]);
	assert_eq!(snapshot.worktrees.len(), 1);
	assert!(snapshot.post_review_lanes.is_empty());
}
