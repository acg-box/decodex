use crate::{
	orchestrator::{
		self, ReviewLifecycleHandoffFixture,
		tests::{self, recovery_terminal_support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn cleanup_completed_post_review_lane_uses_persisted_lifecycle_authority() {
	let cases = [
		(
			"matching current branch",
			"https://github.com/hack-ink/decodex/pull/181",
			"run-closeout-cleanup",
			1,
			"run-closeout-cleanup",
		),
		(
			"stale run identity with current lifecycle authority",
			"https://github.com/hack-ink/decodex/pull/182",
			"run-closeout-stale",
			7,
			"run-closeout-current",
		),
	];

	for (case_name, pr_url, marker_run_id, marker_attempt, issue_run_id) in cases {
		let (temp_dir, base_config, workflow) = tests::temp_project_layout();
		let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
		let issue = tests::sample_issue("Done", &[]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let worktree_manager =
			WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
		let worktree = worktree_manager
			.ensure_worktree(&issue.identifier, false)
			.expect("retained closeout worktree should exist");
		let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
		let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
			&temp_dir, &worktree, pr_url, &head_oid,
		);
		let remote_root =
			config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

		recovery_terminal_support::initialize_closeout_cleanup_origin(
			config.repo_root(),
			&remote_root,
		);
		tests::git_status_success(
			config.repo_root(),
			&["push", "origin", &format!("HEAD:{}", worktree.branch_name)],
		);
		tests::seed_review_lifecycle_handoff_fixture_value(
			&state_store,
			config.service_id(),
			&issue.id,
			&ReviewLifecycleHandoffFixture::new(
				marker_run_id,
				marker_attempt,
				&worktree.branch_name,
				pr_url,
				"main",
				&worktree.branch_name,
				&head_oid,
			),
		);

		state_store
			.upsert_worktree(
				config.service_id(),
				&issue.id,
				&worktree.branch_name,
				&worktree.path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let issue_run =
			recovery_terminal_support::sample_closeout_issue_run(&issue, &worktree, issue_run_id);

		orchestrator::cleanup_completed_post_review_lane(
			&config,
			&workflow,
			&state_store,
			&issue_run,
		)
		.expect("cleanup should use the persisted lifecycle authority");

		assert!(
			!worktree.path.exists(),
			"cleanup should remove the retained worktree path for {case_name}"
		);
	}
}
