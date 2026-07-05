use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn idle_daemon_recovery_reconstructs_completed_closeout_worktree_mapping() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DAEMON_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
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
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	orchestrator::recover_and_reconcile_idle_daemon_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
		None,
	)
	.expect("idle daemon recovery should succeed");

	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"idle daemon recovery should reconstruct retained closeout worktree mappings from disk"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"blocked retained closeout recovery should not invent a live lease without fresh activity"
	);
}
