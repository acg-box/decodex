use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support},
	},
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_leaves_managed_worktree_git_metadata_untouched() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["config", "--local", "codex.github-identity", "y"])
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["config", "--local", "codex.linear-workspace", "hackink"])
			.status()
			.expect("git config should run")
			.success()
	);

	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	review_landing_status_support::remove_local_git_metadata_for_post_review_status(&worktree.path);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "ready_to_land");
	assert_eq!(tests::try_git_local_config_value(&worktree.path, "codex.github-identity"), None);
	assert_eq!(tests::try_git_local_config_value(&worktree.path, "codex.linear-workspace"), None);
	assert_eq!(tests::git_remote_url(&worktree.path, "origin"), None);
}
