use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	worktree::{WorktreeManager, WorktreeSpec},
};

#[test]
fn cleanup_completed_post_review_lane_fails_closed_when_pr_target_branch_drifted() {
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
	let pr_url = "https://github.com/hack-ink/decodex/pull/180";
	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response_with_base_ref(
		&temp_dir,
		&worktree,
		pr_url,
		&head_oid,
		"release/1.x",
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

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: worktree.branch_name.clone(),
			issue_identifier: issue.identifier.clone(),
			path: worktree.path.clone(),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 1,
		run_id: String::from("run-closeout-retargeted-pr"),
		retry_budget_base: 0,
	};
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("cleanup must fail closed when the merged PR target branch drifted");

	assert!(
		error.to_string().contains("expected PR") && error.to_string().contains("release/1.x"),
		"cleanup should surface the authoritative PR target-branch mismatch"
	);
	assert!(
		worktree.path.exists(),
		"cleanup must preserve the retained worktree when the merged PR target branch drifted"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping must remain so cleanup can retry after a corrected handoff"
	);
}
