use crate::{
	agent::app_server::tests::recorder::{
		AppServerDynamicToolFailure, DynamicToolContentItem, JsonRpcRequest, RequestWaitPhase,
		RunRecorder, TempDir, app_server,
	},
	state::{self, StateStore},
};

#[test]
fn dynamic_tool_call_unavailable_outside_turn_execution_is_protocol_diagnostic() {
	let dispatch = app_server::dynamic_tool_call_unavailable_for_phase(RequestWaitPhase::TurnStart);

	assert!(!dispatch.response.success);
	assert!(matches!(
		dispatch.response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("unavailable while waiting for turn/start")
	));
	assert_eq!(
		dispatch.terminal_failure.as_ref().map(AppServerDynamicToolFailure::error_class),
		Some("app_server_dynamic_tool_protocol_failure")
	);

	let diagnostic = dispatch.diagnostic.expect("protocol failure should publish a diagnostic");

	assert_eq!(diagnostic.failure_class, "app_server_dynamic_tool_protocol_failure");
	assert!(diagnostic.message.contains("unavailable while waiting for turn/start"));
	assert_eq!(
		diagnostic.next_action,
		"inspect the declared dynamic tool surface and item/tool/call payload before retrying the lane"
	);
}
#[test]
fn turn_execution_records_dynamic_tool_call_before_response() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let request = JsonRpcRequest {
		id: serde_json::json!(7),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"tool": "issue_progress_checkpoint",
			"arguments": {"phase": "verifying"},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"callId": "call-1",
		}),
	};
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	super::record_server_request(&mut recorder, &request)
		.expect("tool call request should record before handler execution");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let activity = marker.child_agent_activity().expect("child activity should be captured");

	assert_eq!(marker.last_event_type(), Some("item/tool/call"));
	assert_eq!(activity.current_bucket.as_deref(), Some("Tracker"));
	assert_eq!(activity.current_detail.as_deref(), Some("issue_progress_checkpoint"));
	assert!(activity.buckets.iter().any(|bucket| {
		bucket.name == "Tracker" && bucket.tool_call_count == 1 && bucket.event_count == 1
	}));
}
