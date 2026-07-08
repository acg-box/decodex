use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker, recovery_terminal_support},
	},
	worktree::WorktreeManager,
};

#[test]
fn blocks_closeout_on_merge_readback_conflict() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/204";
	let _path_guard = recovery_terminal_support::install_fake_closeout_gh_responses_with_states(
		&temp_dir, &worktree, pr_url, &head_oid, "OPEN", "MERGED",
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
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

	review_state.state = String::from("MERGED");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "pull_request_merge_state_conflict");
	assert_eq!(lanes[0].readback_warning.as_deref(), Some("pull_request_merge_state_conflict"));
	assert_eq!(lanes[0].readback_root_cause.as_deref(), Some("lineage_validation_failed"));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("OPEN"));
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}
