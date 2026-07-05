use std::fs;

use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::{self, StateStore},
	tracker,
};

#[test]
fn stalled_idle_duration_ignores_future_last_activity() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let run_id = "run-future-activity";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");

	assert_eq!(
		orchestrator::stalled_idle_duration(
			&state_store,
			&state_store
				.run_attempt(run_id)
				.expect("run lookup should succeed")
				.expect("run attempt should exist"),
			None,
			last_activity - 1
		)
		.expect("idle duration should evaluate"),
		None
	);
}

#[test]
fn exited_child_reconciliation_defers_dirty_worktree_with_retry_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-failed-with-retry-marker";
	let worktree_path = config.worktree_root().join("PUB-101-retry-marker");

	tests::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-101-retry-marker",
			".worktrees/PUB-101-retry-marker",
			"main",
		],
	);
	fs::write(worktree_path.join("README.md"), "retry still owns this patch\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101-retry-marker",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_retry_schedule(&worktree_path, run_id, 1, "failure", 1)
		.expect("retry schedule marker should write");

	state_store
		.append_event(run_id, 1, "turn/completed", "{\"status\":\"completed\"}")
		.expect("protocol event should record");

	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		run_id,
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child reconciliation should evaluate");

	assert!(
		actions.is_empty(),
		"retry marker should keep failed dirty worktree under retry scheduler ownership"
	);
}
