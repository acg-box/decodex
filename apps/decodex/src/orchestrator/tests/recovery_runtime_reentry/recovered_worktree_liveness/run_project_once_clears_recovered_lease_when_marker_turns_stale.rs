use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_clears_recovered_lease_when_marker_turns_stale() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-1", 1)
		.expect("fresh activity marker should write");

	let initial_summary =
		orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
			.expect("initial recovery should succeed");

	assert!(
		initial_summary.is_none(),
		"fresh recovered activity should block redispatch and reconstruct the live lease"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("recovered lease should exist")
			.run_id(),
		"run-1"
	);

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, u32::MAX)
		.expect("stale activity marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("stale recovery should succeed")
		.expect("stale recovered lease should no longer block retry planning");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"stale recovered markers should clear the reconstructed lease before retry planning"
	);
}
