use crate::{
	agent::app_server::tests::recorder::{RunRecorder, TempDir},
	state::{self, StateStore},
};

#[test]
fn recorder_treats_item_started_as_model_execution_wait() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"turn/started",
			r#"{"method":"turn/started","params":{"turn":{"id":"turn-1","status":"running"}}}"#,
		)
		.expect("turn start should record");
	recorder
		.record(
			"item/started",
			r#"{"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"item-1","kind":"agentReasoning"}}}"#,
		)
		.expect("item start should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.turn_status.as_deref(), Some("running"));
	assert_eq!(summary.waiting_reason.as_deref(), Some("model_execution"));
}

#[test]
fn recorder_does_not_treat_tool_item_started_as_model_execution_wait() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"item/tool/call",
			r#"{"method":"item/tool/call","params":{"threadId":"thread-1","turnId":"turn-1","tool":"shell","arguments":{"cmd":"sleep 60"}}}"#,
		)
		.expect("tool call should record");
	recorder
		.record(
			"item/started",
			r#"{"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"item-1","type":"toolCall"}}}"#,
		)
		.expect("tool item start should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.waiting_reason.as_deref(), Some("tool_execution"));
}

#[test]
fn recorder_does_not_let_tool_item_started_inherit_model_execution_wait() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"turn/started",
			r#"{"method":"turn/started","params":{"turn":{"id":"turn-1","status":"running"}}}"#,
		)
		.expect("turn start should record");
	recorder
		.record(
			"item/started",
			r#"{"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"item-1","type":"commandExecution"}}}"#,
		)
		.expect("command item start should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.turn_status.as_deref(), Some("running"));
	assert_eq!(summary.waiting_reason.as_deref(), Some("tool_execution"));
}
