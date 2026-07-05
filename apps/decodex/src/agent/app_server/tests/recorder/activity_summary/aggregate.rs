use crate::{
	agent::app_server::tests::recorder::{RunRecorder, TempDir},
	state::{self, StateStore},
};

#[test]
fn recorder_aggregates_child_agent_activity_breakdown() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let large_output = "x".repeat(100_500);
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"thread/status/changed",
			r#"{"method":"thread/status/changed","params":{"threadId":"thread-1","status":{"type":"active"}}}"#,
		)
		.expect("thread status should record");
	recorder
		.record(
			"item/tool/call",
			r#"{"method":"item/tool/call","params":{"tool":"functions.exec_command","arguments":{"cmd":"cargo make test"},"threadId":"thread-1","turnId":"turn-1","callId":"call-1"}}"#,
		)
		.expect("shell tool call should record");
	recorder
		.record(
			"item/tool/call/response",
			r#"{"contentItems":[{"type":"inputText","text":"tests passed"}],"success":true}"#,
		)
		.expect("shell tool response should record");

	for call_id in ["call-2", "call-3"] {
		recorder
			.record(
				"item/tool/call",
				&format!(
					r#"{{"method":"item/tool/call","params":{{"tool":"view_image","arguments":{{"detail":"original"}},"threadId":"thread-1","turnId":"turn-1","callId":"{call_id}"}}}}"#
				),
			)
			.expect("image tool call should record");
		recorder
			.record(
				"item/tool/call/response",
				&format!(
					r#"{{"contentItems":[{{"type":"inputText","text":"{large_output}"}}],"success":true}}"#
				),
			)
			.expect("image tool response should record");
	}

	recorder
		.record(
			"turn/completed",
			r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"completed"},"usage":{"input_tokens":105000,"output_tokens":12000}}}"#,
		)
		.expect("turn completion should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.child_agent_activity().expect("child activity should be captured");
	let protocol_activity =
		marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.event_count, 8);
	assert_eq!(summary.tool_call_count, 3);
	assert_eq!(summary.current_bucket, None);
	assert_eq!(summary.input_tokens_current, Some(105_000));
	assert_eq!(summary.input_tokens_cumulative, 105_000);
	assert_eq!(summary.output_tokens_cumulative, 12_000);
	assert_eq!(summary.largest_tool_output_tool.as_deref(), Some("view_image"));
	assert!(
		summary
			.large_output_warnings
			.iter()
			.any(|warning| warning.contains("view_image repeated 2 large outputs"))
	);
	assert!(summary.buckets.iter().any(|bucket| {
		bucket.name == "Shell" && bucket.tool_call_count == 1 && bucket.event_count >= 2
	}));
	assert!(summary.buckets.iter().any(|bucket| {
		bucket.name == "Browser/Image"
			&& bucket.tool_call_count == 2
			&& bucket.output_bytes > 200_000
	}));
	assert!(summary.buckets.iter().any(|bucket| {
		bucket.name == "Model" && bucket.input_tokens == 105_000 && bucket.output_tokens == 12_000
	}));
	assert_eq!(protocol_activity.turn_status.as_deref(), Some("completed"));
	assert_eq!(protocol_activity.waiting_reason.as_deref(), Some("turn_completed"));
	assert_eq!(protocol_activity.recent_events.len(), 8);
	assert!(protocol_activity.recent_events.iter().any(|event| {
		event.event_type == "item/tool/call"
			&& event.detail.as_deref() == Some("functions.exec_command")
	}));
	assert!(protocol_activity.recent_events.iter().any(|event| {
		event.event_type == "turn/completed"
			&& event.category == "turn"
			&& event.detail.as_deref() == Some("completed")
	}));
}
