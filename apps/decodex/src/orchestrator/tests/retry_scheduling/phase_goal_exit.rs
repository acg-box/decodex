use std::{fs, process::Command};

use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, CONTINUATION_PENDING_RUN_STATUS, ChildExitRetryContext, ChildRunRef,
		IssueDispatchMode, RetryQueue, StateStore, tests,
		tests::{FakeTracker, TEST_SERVICE_ID, retry_scheduling::support},
	},
	state,
};

#[test]
fn schedule_retry_after_child_exit_records_continuation_retry_for_clean_exit() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, CONTINUATION_PENDING_RUN_STATUS)
		.expect("run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("continuation retry should schedule");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 1)
		.expect("private continuation lineage events should load");

	assert_eq!(entry.kind, orchestrator::RetryKind::Continuation);
	assert_eq!(entry.attempt, 1);
	assert!(events.iter().any(|event| {
		event.event_type() == "continuation_lineage"
			&& event.payload()["continuation_of_run_id"] == run_id
			&& event.payload()["retry_budget_consumed"] == false
			&& event.payload()["next_retry_kind"] == "continuation"
	}));
}

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
			&& comment.contains("Source failure class `repo_gate_tracked_rewrites_left`")
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

#[test]
fn schedule_retry_after_child_exit_preserves_specific_retry_schedule_kind_for_failure_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_retry_schedule(
		&worktree_path,
		run_id,
		1,
		"git_lock_contention",
		OffsetDateTime::now_utc().unix_timestamp() + 30,
	)
	.expect("specific retry schedule should write");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("failure retry should schedule");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");
	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(marker.retry_kind(), Some("git_lock_contention"));
}

#[test]
fn schedule_retry_after_child_exit_retains_continuation_retry_for_stale_startable_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("Todo");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, CONTINUATION_PENDING_RUN_STATUS)
		.expect("run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("continuation retry should tolerate a stale startable tracker reread");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Continuation);
	assert_eq!(entry.attempt, 1);
}

#[test]
fn schedule_retry_after_child_exit_skips_retry_for_completed_successful_run() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("completed run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("completed successful runs should not schedule another retry");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"successful review-handoff style exits must not reopen the same run as a continuation"
	);
}

#[test]
fn schedule_retry_after_child_exit_requires_exact_run_id() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("retry scheduling should succeed");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"retry scheduling should ignore a different run that only matches the issue and attempt"
	);
}
