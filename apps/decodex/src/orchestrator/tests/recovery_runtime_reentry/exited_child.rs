use std::fs;

use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::{self, ProtocolActivityMarker, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn detects_partial_progress_from_dirty_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-stalled-dirty-after-exit",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "run-stalled-dirty-after-exit";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-206", ".worktrees/PUB-206", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained partial work\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run should exit as failed before daemon inspects it");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-206",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id,
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "turn/diff/updated",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let last_protocol_activity =
		state::read_run_protocol_activity_marker(&worktree_path, run_id, 1)
			.expect("protocol marker should read")
			.expect("protocol activity should exist");
	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		run_id,
		last_protocol_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
			if idle_for >= RUN_LEASE_IDLE_TIMEOUT
	));
}

#[test]
fn exited_child_reconciliation_detects_stalled_failed_runs_from_protocol_idle() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-stalled-after-exit",
		"PUB-205",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "run-stalled-after-exit";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-205",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run should exit as failed before daemon inspects it");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id,
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "thread/status/changed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let last_protocol_activity =
		state::read_run_protocol_activity_marker(&worktree_path, run_id, 1)
			.expect("protocol marker should read")
			.expect("protocol activity should exist");
	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		run_id,
		last_protocol_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == issue.id
			&& matches!(
				action.disposition,
				RunLeaseDisposition::Stalled{ idle_for }
					if idle_for >= RUN_LEASE_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn exited_child_reconciliation_ignores_superseded_failed_run() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-superseded-after-exit",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let stale_run_id = "run-superseded-after-exit-1";
	let newer_run_id = "run-superseded-after-exit-2";
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 1, "failed")
		.expect("stale run should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 2, "running")
		.expect("newer run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, newer_run_id, "In Progress")
		.expect("newer lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-206",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: stale_run_id,
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "thread/status/changed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		stale_run_id,
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		&actions[0].disposition,
		RunLeaseDisposition::Superseded {
			newer_run_id: observed_run_id,
			newer_attempt_number: 2,
		} if observed_run_id == newer_run_id
	));

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("superseded reconciliation should succeed");

	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("newer lease should remain");

	assert_eq!(lease.run_id(), newer_run_id);
	assert!(
		tracker.comments.borrow().is_empty(),
		"superseded stale child must not write needs-attention comments"
	);
}
