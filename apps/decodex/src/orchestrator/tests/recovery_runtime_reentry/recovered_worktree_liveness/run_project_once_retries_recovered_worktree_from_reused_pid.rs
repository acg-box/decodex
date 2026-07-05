use std::process::{self};

use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_project_once_retries_recovered_worktree_from_reused_pid() {
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

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, process::id())
		.expect("activity marker should write");
	tests::rewrite_run_activity_marker_process_start_identity(
		&worktree.path,
		"previous-process-start",
	);

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("same-boot PID-reuse recovery should succeed")
		.expect("same-boot PID-reuse recovered lane should be selected for retry");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"PID reuse must not reconstruct live leases when the process start identity changed"
	);
}
