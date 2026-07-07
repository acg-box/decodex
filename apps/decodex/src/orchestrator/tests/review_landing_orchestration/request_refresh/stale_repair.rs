use crate::orchestrator::{
	self, StateStore,
	tests::{
		self, FakePullRequestReviewStateInspector, FakeTracker,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, review_landing_status_support,
	},
};

#[test]
fn reconcile_post_review_orchestration_rebinds_stale_head_lifecycle_authority_after_repair_push() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&config, "PATH");
	let _path_guard = tests::install_fake_post_issue_comment_gh_response(
		&temp_dir,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		"2025-11-03T00:00:00Z",
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let marker_head_oid = tests::git_output(&repo_root, &["rev-parse", "HEAD"]);
	let current_head_oid =
		tests::commit_worktree_change(&repo_root, "repair.txt", "repair push\n", "repair push");

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &marker_head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_transition_fixture(
			"main",
			pr_url,
			&marker_head_oid,
			"waiting_for_result",
			1,
		),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&current_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should rebind stale lifecycle authority without attention");

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_ack");
	assert_eq!(marker.head_sha(), current_head_oid);
	assert_eq!(marker.request_comment_database_id(), Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID));
	assert_eq!(marker.external_round_count(), 1);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}
