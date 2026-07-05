use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker, recovery_terminal_support},
	},
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_keeps_merged_closeout_visible_after_landed_main_advances() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let pr_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let merge_commit_oid = tests::commit_worktree_change(
		&worktree.path,
		"landed.txt",
		"landed\n",
		"land retained lane",
	);
	let current_head_oid =
		tests::commit_worktree_change(&worktree.path, "later.txt", "later\n", "advance main later");
	let pr_url = "https://github.com/hack-ink/decodex/pull/203";
	let _path_guard = recovery_terminal_support::install_fake_closeout_gh_responses(
		&temp_dir,
		&worktree,
		pr_url,
		&pr_head_oid,
	);

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
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &pr_head_oid),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&pr_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");
	review_state.merge_commit_oid = Some(merge_commit_oid);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(tests::git_output(&worktree.path, &["rev-parse", "HEAD"]), current_head_oid);
	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "continue");
	assert_eq!(lanes[0].reason, "pull_request_merged_closeout_pending");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("MERGED"));
}
