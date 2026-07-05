use std::fs;

use tempfile::TempDir;

use crate::state::{RunControlActionRequest, StateStore, tests::IN_PROGRESS_STATE};

#[test]
fn run_control_rejects_stale_turn_and_run_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-current", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.record_run_attempt("run-current", "issue-1", 1, "running")
		.expect("attempt should record");
	store.update_run_thread("run-current", "thread-1").expect("thread should record");
	store.update_run_turn("run-current", "turn-current").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt(
			"run-current",
			1,
			&channel_path,
			"local_file",
		)
		.expect("control channel should publish");

	let stale_turn = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-current",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-old"),
			source: "test_hook",
			action: "steer",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("stale turn should be audited");
	let stale_run = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-old",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-current"),
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("stale run should be audited");

	assert_eq!(stale_turn.outcome(), "rejected");
	assert_eq!(stale_turn.reason(), "turn_mismatch");
	assert_eq!(stale_turn.current_turn_id(), Some("turn-current"));
	assert_eq!(stale_run.outcome(), "rejected");
	assert_eq!(stale_run.reason(), "run_not_found");

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-current", 1)
		.expect("private control audit should read");
	let stale_turn_event = events
		.iter()
		.find(|event| event.record_id() == stale_turn.audit_record_id())
		.expect("stale turn audit event should exist");

	assert_eq!(
		stale_turn_event.payload()["failure_class"].as_str(),
		Some("stale_expected_turn_id")
	);
	assert_eq!(stale_turn_event.payload()["observed"]["turn_id"].as_str(), Some("turn-current"));
}
