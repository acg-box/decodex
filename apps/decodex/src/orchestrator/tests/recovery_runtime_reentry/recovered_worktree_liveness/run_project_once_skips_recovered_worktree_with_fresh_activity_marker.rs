use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_skips_recovered_worktree_with_fresh_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-1", 1)
		.expect("activity marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"fresh child activity should recover as a current lane instead of redispatching"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should be reconstructed")
			.run_id(),
		"run-1"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should be reconstructed")
			.status(),
		"running"
	);
}
