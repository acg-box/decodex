use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_clears_stale_completed_closeout_lease_but_keeps_worktree() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_STARTUP_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
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
	let run_id = "run-closeout-startup";
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

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
		"blocked retained closeout should not redispatch during startup recovery"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"startup reconciliation should clear stale completed closeout leases"
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
		"startup reconciliation should preserve the retained closeout worktree mapping"
	);
	assert!(
		worktree.path.exists(),
		"startup reconciliation should leave the retained closeout worktree on disk"
	);
}

#[test]
fn run_project_once_preserves_fresh_completed_closeout_lease() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_STARTUP_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
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
	let run_id = "run-closeout-fresh-startup";
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, &issue.state.name)
		.expect("fresh lease should record");
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
		"fresh retained closeout activity should block redispatch during startup recovery"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("fresh retained closeout lease should survive")
			.run_id(),
		run_id
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should still exist")
			.status(),
		"running"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"startup reconciliation should preserve the retained closeout worktree mapping"
	);
}
