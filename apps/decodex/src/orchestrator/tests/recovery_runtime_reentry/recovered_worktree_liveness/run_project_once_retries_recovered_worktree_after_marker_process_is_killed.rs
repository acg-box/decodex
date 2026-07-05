use std::process::Command;

use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[cfg(unix)]
#[test]
fn run_project_once_retries_recovered_worktree_after_marker_process_is_killed() {
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
	let mut child = Command::new("/bin/sh")
		.args(["-c", "exec sleep 60"])
		.spawn()
		.expect("kill-smoke child process should start");
	let child_process_id = child.id();

	assert!(
		orchestrator::process_is_alive(child_process_id),
		"kill-smoke child process should be live before marker write"
	);

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, child_process_id)
		.expect("activity marker should write");

	child.kill().expect("kill-smoke child process should be killed");
	child.wait().expect("kill-smoke child process should be reaped");

	assert!(
		!orchestrator::process_is_alive(child_process_id),
		"kill-smoke child process should no longer be live after kill"
	);

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("kill-smoke recovery should succeed")
		.expect("killed-process recovered lane should be selected for retry");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"killed marker processes must not reconstruct live leases"
	);
}
