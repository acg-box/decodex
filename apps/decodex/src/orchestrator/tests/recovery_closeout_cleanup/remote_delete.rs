use crate::{
	orchestrator::{
		self, ReviewHandoffMarker,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn cleanup_completed_post_review_lane_preserves_worktree_when_remote_delete_fails() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Done", &[]);
	let _tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");
	let head_oid = tests::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/67";
	let worktree = WorktreeSpec {
		branch_name: String::from("main"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: true,
	};
	let _path_guard =
		recovery_terminal_support::install_fake_merged_pr_gh_response_with_delete_exit_code(
			&temp_dir, &worktree, pr_url, &head_oid, 1,
		);

	recovery_terminal_support::initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewHandoffMarker::new(
			"run-closeout-cleanup",
			1,
			"main",
			pr_url,
			"main",
			"main",
			head_oid,
		),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"main",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");

	let issue_run = recovery_terminal_support::sample_closeout_issue_run(
		&issue,
		&worktree,
		"run-closeout-cleanup",
	);
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("remote branch delete failures must stop cleanup");

	assert!(
		error.to_string().contains("Failed to delete retained remote branch"),
		"cleanup should surface the remote delete failure"
	);
	assert!(
		config.repo_root().exists(),
		"cleanup must preserve the retained worktree when remote delete fails"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping must remain so cleanup can retry later"
	);
	assert!(
		!tests::git_output(config.repo_root(), &["branch", "--list", "main"]).is_empty(),
		"cleanup must not mutate local branch state when remote delete fails before local cleanup"
	);
}
