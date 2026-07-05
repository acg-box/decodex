use std::fs;

use tempfile::TempDir;

use crate::state::{RunControlActionRequest, StateStore, tests::IN_PROGRESS_STATE};

#[test]
fn run_control_rejects_missing_channel_file() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish");

	fs::remove_file(&channel_path).expect("control channel should be removable");

	let receipt = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("missing channel should be audited");

	assert_eq!(receipt.outcome(), "rejected");
	assert_eq!(receipt.reason(), "control_channel_missing");
}
