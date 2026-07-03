use std::process::{self, Command};

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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_project_once_retries_recovered_worktree_from_previous_boot() {
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
	tests::rewrite_run_activity_marker_host_boot_id(&worktree.path, "previous-boot");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("previous-boot recovery should succeed")
		.expect("previous-boot recovered lane should be selected for retry");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"previous-boot markers must not reconstruct live leases even when the PID exists"
	);
}

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

#[cfg(unix)]
#[test]
fn process_is_alive_handles_current_process_and_invalid_sentinel() {
	assert!(
		orchestrator::process_is_alive(process::id()),
		"current process should always be reported as alive"
	);
	assert!(
		!orchestrator::process_is_alive(u32::MAX),
		"sentinel pid values should never be treated as live processes"
	);
}

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
