use std::{env, fs, time::Duration};

use tempfile::TempDir;

use crate::{
	agent::app_server::{self, AppServerTurnFailure, tests},
	state::{self, StateStore},
	test_support::TestEnvVarGuard,
};

#[test]
fn turn_completion_ignores_orphan_json_rpc_response() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let marker_path = temp_dir.path().join("activity");
	let fake_bin_dir =
		tests::install_fake_codex_script(&temp_dir, tests::orphan_response_fake_codex_script());
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("orphan-response-run");
	request.issue_id = String::from("orphan-response-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);
	request.activity_marker_path = Some(marker_path.clone());

	let result = app_server::execute_app_server_run(&request, &state_store)
		.expect("orphan response during turn wait should not fail the run");

	assert_eq!(result.thread_id, "thread-1");
	assert_eq!(result.turn_id, "turn-1");
	assert_eq!(result.final_output, "ORPHAN_OK");

	let marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let protocol_activity =
		marker.protocol_activity().expect("protocol activity should be captured");

	assert!(state_store.event_count(&request.run_id).expect("event count should load") > 0);
	assert!(
		protocol_activity.recent_events.iter().any(|event| event.event_type == "json-rpc/response")
	);
	assert_eq!(marker.last_event_type(), Some("turn/completed"));
}

#[test]
fn turn_completion_waits_through_retrying_error_notification() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let marker_path = temp_dir.path().join("activity");
	let fake_bin_dir =
		tests::install_fake_codex_script(&temp_dir, &tests::retrying_error_fake_codex_script());
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("retrying-error-run");
	request.issue_id = String::from("retrying-error-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);
	request.activity_marker_path = Some(marker_path.clone());

	let result = app_server::execute_app_server_run(&request, &state_store)
		.expect("retrying error during turn wait should not fail the run");

	assert_eq!(result.thread_id, "thread-1");
	assert_eq!(result.turn_id, "turn-1");
	assert_eq!(result.final_output, "ORPHAN_OK");

	let marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_event_type(), Some("turn/completed"));
	assert!(
		state_store
			.run_has_protocol_event(&request.run_id, "error")
			.expect("retrying error event lookup should load")
	);
}

#[test]
fn app_server_run_records_interrupted_turn_without_error_as_structured_failure() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let fake_bin_dir = tests::install_fake_codex_script(
		&temp_dir,
		&tests::interrupted_without_error_fake_codex_script(),
	);
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("interrupted-no-error-run");
	request.issue_id = String::from("interrupted-no-error-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);

	let error = app_server::execute_app_server_run(&request, &state_store)
		.expect_err("interrupted turn without error payload should fail the run");
	let failure =
		error.downcast_ref::<AppServerTurnFailure>().expect("error should be a turn failure");
	let attempt = state_store
		.run_attempt(&request.run_id)
		.expect("run attempt lookup should load")
		.expect("run attempt should exist");

	assert_eq!(failure.error_class(), "app_server_turn_missing_error_payload");
	assert!(failure.to_string().contains("status `interrupted`"));
	assert_eq!(attempt.status(), "failed");
	assert_eq!(attempt.thread_id(), Some("thread-1"));
	assert_eq!(attempt.turn_id(), Some("turn-1"));
	assert!(
		state_store
			.run_has_protocol_event(&request.run_id, "turn/completed")
			.expect("turn completed event lookup should load")
	);
}
