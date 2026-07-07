use std::path::PathBuf;

use crate::{
	orchestrator::{
		self,
		tests::{
			FakeTracker, TEST_SERVICE_ID, recovery_terminal_support, {self},
		},
	},
	state::StateStore,
	tracker::{self},
	worktree::{WorktreeManager, WorktreeSpec},
};

#[test]
fn closeout_completed_state_check_skips_redundant_transition() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = recovery_terminal_support::issue_with_completed_state(tests::sample_issue(
		"Done",
		&[active_label.as_str()],
	));
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: PathBuf::from(".worktrees/PUB-101"),
		reused_existing: true,
	};
	let issue_run = recovery_terminal_support::sample_closeout_issue_run(
		&issue,
		&worktree,
		"pub-101-closeout-done",
	);

	orchestrator::ensure_closeout_issue_completed_state(&tracker, &workflow, &issue_run)
		.expect("completed issue should not require another transition");

	assert!(
		tracker.state_updates.borrow().is_empty(),
		"already completed issues should not be transitioned again"
	);
}

#[test]
fn closeout_dispatch_validates_pr_before_marking_issue_done() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = recovery_terminal_support::issue_with_completed_state(tests::sample_issue(
		"In Review",
		&[active_label.as_str()],
	));
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/702";
	let _path_guard = recovery_terminal_support::install_fake_closeout_gh_responses_with_state(
		&temp_dir, &worktree, pr_url, &head_oid, "OPEN",
	);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	recovery_terminal_support::initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	recovery_terminal_support::route_origin_github_url_to_local_bare_repo(
		config.repo_root(),
		&remote_root,
	);
	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let issue_run = recovery_terminal_support::sample_closeout_issue_run(
		&issue,
		&worktree,
		"pub-702-attempt-1-closeout",
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "starting")
		.expect("run attempt should record");

	let error =
		orchestrator::execute_issue_run(&tracker, &config, &workflow, &state_store, issue_run)
			.expect_err("unmerged PR should stop deterministic closeout");

	assert!(
		error.to_string().contains("must be merged before closeout completes"),
		"closeout should fail at PR validation: {error:?}"
	);
	assert!(
		tracker.state_updates.borrow().is_empty(),
		"closeout must not mark the issue done before PR validation succeeds"
	);
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex closeout completed")),
		"closeout must not write a closeout completion record when PR validation fails"
	);
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"closeout PR visibility races should remain retryable instead of terminal"
	);
	assert!(worktree.path.exists(), "failed closeout should preserve the retained worktree");
}
