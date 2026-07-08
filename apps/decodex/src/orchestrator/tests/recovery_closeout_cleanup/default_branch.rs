use std::fs;

use crate::{
	orchestrator::{
		self,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, recovery_terminal_support,
		},
	},
	state::StateStore,
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn reports_cleanup_blocker_after_dirty_default_branch() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = tests::sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/119";
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

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	fs::write(config.repo_root().join("README.md"), "local repo override\n")
		.expect("tracked repo-root file should become dirty");

	let issue_run = recovery_terminal_support::sample_closeout_issue_run(
		&issue,
		&worktree,
		"run-closeout-dirty-root",
	);
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("tracked repo-root dirtiness must block default-branch sync");

	assert!(
		error.to_string().contains("tracked local changes"),
		"cleanup should surface the default-branch sync blocker: {error:?}"
	);
	assert!(worktree.path.exists(), "cleanup must preserve the retained worktree");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-closeout-dirty-root-{attempt}"),
				&issue.id,
				attempt,
				"failed",
			)
			.expect("failed closeout attempt should record");
	}

	let mut merged_review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(merged_review_state)]),
	)
	.expect("post-review status should classify the retained cleanup blocker");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "cleanup_blocked");
	assert_eq!(lanes[0].reason, "default_branch_worktree_dirty");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("MERGED"));
}
