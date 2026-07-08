use crate::orchestrator::{
	self, StateStore, TERMINAL_GUARDED_RUN_STATUS,
	tests::{
		self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
	},
};

#[test]
fn filters_terminal_identifier_worktree_before_refresh() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("Todo");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = config.worktree_root().join(stale_issue_id);

	state_store
		.record_run_attempt("run-01", stale_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("valid worktree should record");
	state_store
		.upsert_worktree(
			"pubfi",
			stale_issue_id,
			"x/pubfi-pub-001",
			&stale_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should ignore stale terminal local residue");

	let refresh_queries = tracker.refresh_queries.borrow();

	assert!(
		refresh_queries.iter().flatten().any(|issue_id| issue_id == &issue.id),
		"valid worktree issue id should still be sent to tracker refresh"
	);
	assert!(
		refresh_queries.iter().flatten().all(|issue_id| issue_id != stale_issue_id),
		"terminal local identifier residue must not be sent to post-review tracker refresh"
	);
}
