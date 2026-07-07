use crate::{
	orchestrator::{
		self, ReviewLifecycleTransitionFixture, StateStore,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, review_landing_status_support,
		},
	},
	test_support,
};

#[test]
fn reconcile_post_review_orchestration_requests_external_review_without_thumbs_up_baseline() {
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
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewLifecycleTransitionFixture::new(
			"run-1",
			1,
			"main",
			pr_url,
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let initial_review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
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
		&FakePullRequestReviewStateInspector::new(vec![Ok(initial_review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_ack");
	assert_eq!(marker.request_description_thumbs_up_count(), None);
}

#[test]
fn reconcile_post_review_orchestration_uses_matching_handoff_record_for_current_branch() {
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
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let current_branch = "main";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			current_branch,
			&repo_root.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture(current_branch, pr_url, &head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewLifecycleTransitionFixture::new(
			"run-1",
			1,
			current_branch,
			pr_url,
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		current_branch,
		&head_oid,
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
	.expect("post-review orchestration should succeed");

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_ack");
	assert_eq!(marker.pr_url(), pr_url);
}
