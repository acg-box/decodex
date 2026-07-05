use std::{fs, process};

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::{self, RUN_OPERATION_REPO_GATE, StateStore},
};

#[test]
fn run_lease_reconciliation_defers_live_repo_gate_even_with_dirty_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-live-repo-gate";
	let worktree_path = config.worktree_root().join("PUB-101-repo-gate");

	tests::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-101-repo-gate",
			".worktrees/PUB-101-repo-gate",
			"main",
		],
	);
	fs::write(worktree_path.join("README.md"), "repo gate is still running\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101-repo-gate",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_operation_marker_for_process(
		&worktree_path,
		run_id,
		1,
		process::id(),
		RUN_OPERATION_REPO_GATE,
	)
	.expect("live repo-gate marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker should load")
		.expect("marker should exist");
	let last_activity = marker.last_activity_unix_epoch().expect("marker should record activity");
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.is_empty(),
		"live repo gate operation should retain scheduler ownership instead of attention"
	);

	state::write_run_operation_marker_for_process(
		&worktree_path,
		run_id,
		1,
		u32::MAX,
		RUN_OPERATION_REPO_GATE,
	)
	.expect("stopped repo-gate marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker should reload")
		.expect("marker should exist");
	let last_activity = marker.last_activity_unix_epoch().expect("marker should record activity");
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("run lease inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == issue.id
			&& matches!(
				action.disposition,
				RunLeaseDisposition::StalledRetainedPartialProgress{ idle_for }
					if idle_for >= RUN_LEASE_IDLE_TIMEOUT
			)
	}));
}
