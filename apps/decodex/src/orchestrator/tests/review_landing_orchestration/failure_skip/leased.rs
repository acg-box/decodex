use crate::{
	orchestrator::{
		self, StateStore,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
		},
	},
	test_support,
};

#[test]
fn reconcile_post_review_orchestration_skips_issue_with_run_lease() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
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
		&tests::sample_review_lifecycle_transition_fixture(
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	assert!(
		state_store
			.try_acquire_lease(
				config.service_id(),
				&issue.id,
				"active-repair-run",
				workflow.frontmatter().tracker().in_progress_state(),
			)
			.expect("run lease should acquire")
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should skip active repair lanes");

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_result");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}
