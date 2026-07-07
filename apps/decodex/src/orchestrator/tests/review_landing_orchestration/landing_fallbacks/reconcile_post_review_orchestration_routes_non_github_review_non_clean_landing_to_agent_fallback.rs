use crate::orchestrator::{
	self, ReviewLevel, StateStore,
	tests::{
		self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
	},
};

#[test]
fn reconcile_post_review_orchestration_routes_non_github_review_non_clean_landing_to_agent_fallback()
 {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"HAS_HOOKS",
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

	assert_eq!(marker.phase(), "repair_required");
	assert!(
		!invocation_log_path.exists(),
		"non-clean non-GitHub-review retained landing must not invoke runtime admin merge"
	);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}
