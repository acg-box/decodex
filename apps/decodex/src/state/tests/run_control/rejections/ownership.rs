use std::fs;

use tempfile::TempDir;

use crate::state::{RunControlActionRequest, StateStore, tests::IN_PROGRESS_STATE};

#[test]
fn run_control_requires_run_lease_ownership() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");
	let worktree_path = temp_dir.path().join("PUB-101");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			"issue-1",
			"x/pubfi-issue-1",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish");
	store.clear_lease("issue-1").expect("lease should clear");

	let no_lease = store
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
		.expect("missing lease should be audited");

	store
		.upsert_lease("pubfi", "issue-1", "run-other", IN_PROGRESS_STATE)
		.expect("other lease should record");

	let wrong_run = store
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
		.expect("wrong run lease should be audited");

	assert_eq!(no_lease.outcome(), "rejected");
	assert_eq!(no_lease.reason(), "run_lease_missing");
	assert_eq!(wrong_run.outcome(), "rejected");
	assert_eq!(wrong_run.reason(), "run_lease_mismatch");

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-1", 1)
		.expect("private control audit should read");
	let no_lease_event = events
		.iter()
		.find(|event| event.record_id() == no_lease.audit_record_id())
		.expect("missing lease audit event should exist");
	let expected_worktree_path = worktree_path.display().to_string();
	let expected_channel_path = channel_path.display().to_string();

	assert_eq!(no_lease_event.payload()["lane"]["run_lease"].as_bool(), Some(false));
	assert_eq!(no_lease_event.payload()["lane"]["attempt_status"].as_str(), Some("running"));
	assert_eq!(no_lease_event.payload()["lane"]["branch"].as_str(), Some("x/pubfi-issue-1"));
	assert_eq!(
		no_lease_event.payload()["lane"]["worktree_path"].as_str(),
		Some(expected_worktree_path.as_str())
	);
	assert_eq!(no_lease_event.payload()["channel"]["status"].as_str(), Some("active"));
	assert_eq!(no_lease_event.payload()["channel"]["path_exists"].as_bool(), Some(true));
	assert_eq!(
		no_lease_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
}
