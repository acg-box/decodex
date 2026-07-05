use std::process;

use tempfile::TempDir;

use crate::state::{self, EffectiveRuntimeMarker, ProtocolActivityMarker, RUN_OPERATION_REPO_GATE};

#[test]
fn run_operation_marker_resets_stale_per_attempt_fields_on_new_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("first activity marker should write");
	state::write_run_thread_marker(temp_dir.path(), "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(temp_dir.path(), "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		temp_dir.path(),
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnUserInput")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		temp_dir.path(),
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "dangerFullAccess",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 3,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 123)
		.expect("retry schedule should write");
	state::write_run_retry_budget_attempt_count(temp_dir.path(), "run-1", 1, 2)
		.expect("retry budget should write");
	state::write_run_operation_marker(temp_dir.path(), "run-2", 2, RUN_OPERATION_REPO_GATE)
		.expect("next attempt operation marker should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-2");
	assert_eq!(marker.attempt_number(), 2);
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_REPO_GATE));
	assert!(marker.last_progress_unix_epoch().is_some());
	assert_eq!(marker.thread_id(), None);
	assert_eq!(marker.turn_id(), None);
	assert_eq!(marker.thread_status(), None);
	assert!(marker.thread_active_flags().is_empty());
	assert_eq!(marker.event_count(), 0);
	assert_eq!(marker.last_event_type(), None);
	assert_eq!(marker.protocol_activity(), None);
	assert_eq!(marker.effective_model(), None);
	assert_eq!(marker.effective_model_provider(), None);
	assert_eq!(marker.effective_cwd(), None);
	assert_eq!(marker.effective_approval_policy(), None);
	assert_eq!(marker.effective_approvals_reviewer(), None);
	assert_eq!(marker.effective_sandbox_mode(), None);
	assert_eq!(marker.last_protocol_activity_unix_epoch(), None);
	assert_eq!(marker.retry_kind(), None);
	assert_eq!(marker.retry_ready_at_unix_epoch(), None);
	assert_eq!(
		state::read_run_retry_budget_attempt_count(temp_dir.path())
			.expect("retry budget count should load"),
		Some(2)
	);
}
