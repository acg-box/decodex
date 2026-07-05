use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn cleanup_completed_post_review_lane_deletes_local_lane_branch_after_worktree_cleanup() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Done", &[]);
	let _tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";
	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	recovery_terminal_support::initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);

	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let issue_run = recovery_terminal_support::sample_closeout_issue_run(
		&issue,
		&worktree,
		"run-closeout-local-branch-cleanup",
	);

	orchestrator::cleanup_completed_post_review_lane(&config, &workflow, &state_store, &issue_run)
		.expect("cleanup should succeed once merged closeout is authoritative");

	assert!(!worktree.path.exists(), "cleanup should remove the retained worktree path");
	assert!(
		tests::git_output(config.repo_root(), &["branch", "--list", &worktree.branch_name])
			.is_empty(),
		"cleanup should delete the retained local lane branch"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"cleanup should clear retained worktree state after local branch deletion succeeds"
	);
}
