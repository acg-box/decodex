use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_skips_recovered_worktree_without_service_active_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("foreign retained worktree should exist");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"recovery should skip retained worktrees that are not explicitly owned by this service"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"foreign retained worktrees should not be reconstructed into local service state"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"foreign retained worktrees should not rebuild service leases"
	);
}
