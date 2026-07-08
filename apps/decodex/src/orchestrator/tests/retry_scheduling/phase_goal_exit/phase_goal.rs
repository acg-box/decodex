use std::{fs, process::Command};

use crate::orchestrator::{
	self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore,
	tests::{self, FakeTracker, TEST_SERVICE_ID, retry_scheduling::support},
};

#[test]
fn schedule_retry_after_child_exit_terminalizes_open_phase_goal_tracked_rewrites() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 1)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > other.txt\"]",
			),
	);
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-3";

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	tests::commit_worktree_change(config.repo_root(), "other.txt", "before\n", "add other file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	for (attempt, recorded_run_id) in [(1, "run-1"), (2, "run-2"), (3, run_id)] {
		state_store
			.record_run_attempt(recorded_run_id, &issue.id, attempt, "failed")
			.expect("failed run attempt should record");
	}

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal event should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("open phase goal tracked rewrites should terminalize cleanly");

	let run_attempt = state_store
		.run_attempt(run_id)
		.expect("run attempt lookup should succeed")
		.expect("run attempt should still exist");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 3)
		.expect("private events should load");
	let comments = tracker.comments.borrow();

	assert!(!retry_queue.entries.contains_key(&issue.id));
	assert_eq!(run_attempt.status(), "failed");
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["disposition"] == "needs_human_attention"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == false
	}));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_recovery"));
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("Source failure class `repo_gate_lane_external_tracked_rewrite`")
	}));
}

#[test]
fn schedule_retry_after_child_exit_respects_terminal_finalize_before_phase_goal_recovery() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-3";

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	for (attempt, recorded_run_id) in [(1, "run-1"), (2, "run-2"), (3, run_id)] {
		state_store
			.record_run_attempt(recorded_run_id, &issue.id, attempt, "failed")
			.expect("failed run attempt should record");
	}

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal event should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"terminal_finalize",
			serde_json::json!({
				"path": "manual_attention",
				"mode": "normal",
				"branch": "x/pubfi-pub-101",
				"worktree_path": config.repo_root().display().to_string(),
			}),
		)
		.expect("terminal finalize event should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("terminalized child exit should keep the terminal path");

	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 3)
		.expect("private events should load");

	assert!(!retry_queue.entries.contains_key(&issue.id));
	assert!(
		events.iter().all(|event| event.event_type() != "phase_goal_recovery"),
		"terminal finalize intent must not be replaced by active phase-goal recovery"
	);
}
