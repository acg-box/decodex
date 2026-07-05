use std::fs;

use crate::{
	orchestrator::{
		self, TERMINAL_GUARD_MARKER_FILE,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_skips_recovered_terminal_guarded_worktree_after_empty_state_startup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue_without_needs_attention_team_label(
		"In Progress",
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	fs::write(
		worktree.path.join(TERMINAL_GUARD_MARKER_FILE),
		"run_id=pub-101-attempt-1-123\nattempt_number=1\n",
	)
	.expect("terminal guard marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"restart recovery should not redispatch retained lanes guarded by a terminal marker"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping should still be reconstructed for guarded retained lanes"
	);
}

#[test]
fn run_project_once_preserves_terminal_recovered_worktree_without_prior_state_when_review_handoff_is_missing()
 {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("terminal retained worktree should be created");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should finish cleanly");

	assert!(
		summary.is_none(),
		"blocked retained closeout with missing handoff should not redispatch during recovery"
	);
	assert!(
		worktree.path.exists(),
		"terminal recovery should preserve the retained closeout worktree on disk for manual intervention"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"terminal recovery should preserve the retained closeout worktree mapping when review handoff is missing"
	);
}

#[test]
fn run_project_once_preserves_completed_unmerged_closeout_worktree() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let run_id = "run-closeout-open-pr-startup";
	let pr_url = "https://github.com/hack-ink/decodex/pull/179";
	let _path_guard = recovery_terminal_support::install_fake_open_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, &issue.state.name)
		.expect("stale lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("startup reconciliation should succeed");

	assert!(
		summary.is_none(),
		"completed retained closeout with an open PR should stay blocked during startup recovery"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"startup reconciliation should clear stale completed closeout leases when the PR is still open"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should still exist")
			.status(),
		"interrupted"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"startup reconciliation should preserve the retained closeout worktree mapping until the PR merges"
	);
	assert!(
		worktree.path.exists(),
		"startup reconciliation should leave the retained closeout worktree on disk while waiting for merge"
	);
}
