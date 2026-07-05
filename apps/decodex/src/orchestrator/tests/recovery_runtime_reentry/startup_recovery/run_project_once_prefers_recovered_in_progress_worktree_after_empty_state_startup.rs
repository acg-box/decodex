use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_prefers_recovered_in_progress_worktree_after_empty_state_startup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovered dry run should succeed")
		.expect("active recovered issue should be selected");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping should be reconstructed from the retained lane"
	);
}
