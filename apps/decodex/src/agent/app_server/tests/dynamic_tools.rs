#[allow(clippy::wildcard_imports)] use super::*;

#[test]
fn dynamic_tool_call_accepts_thread_bound_request_when_payload_turn_id_differs() {
	let handler = NamespacedDynamicToolHandler { seen_namespace: RefCell::new(None) };
	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"namespace": "tracker",
			"threadId": "thread-1",
			"tool": "tracker_tool",
			"turnId": "tool-call-turn"
		}),
	};
	let dispatch = super::super::handle_dynamic_tool_call(
		Some(&handler),
		&request,
		"thread-1",
		Some("active-turn"),
	);

	assert!(dispatch.response.success);
	assert!(dispatch.terminal_failure.is_none());
	assert_eq!(*handler.seen_namespace.borrow(), Some(String::from("tracker")));
}

#[test]
fn dynamic_tool_call_rejects_wrong_thread_even_when_payload_turn_id_differs() {
	let handler = NamespacedDynamicToolHandler { seen_namespace: RefCell::new(None) };
	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"namespace": "tracker",
			"threadId": "thread-2",
			"tool": "tracker_tool",
			"turnId": "tool-call-turn"
		}),
	};
	let dispatch = super::super::handle_dynamic_tool_call(
		Some(&handler),
		&request,
		"thread-1",
		Some("active-turn"),
	);

	assert!(!dispatch.response.success);
	assert_eq!(*handler.seen_namespace.borrow(), None);
	assert_eq!(
		dispatch
			.terminal_failure
			.as_ref()
			.map(super::super::AppServerDynamicToolFailure::error_class),
		Some("app_server_dynamic_tool_protocol_failure")
	);
	assert!(matches!(
		dispatch.response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("targeted thread `thread-2`")
	));
}

#[test]
fn dynamic_tool_call_rejects_invalid_response_shape() {
	let handler = EmptyToolResponseHandler;
	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"threadId": "thread-1",
			"tool": "empty_response",
			"turnId": "turn-1"
		}),
	};
	let dispatch = super::super::handle_dynamic_tool_call(
		Some(&handler),
		&request,
		"thread-1",
		Some("turn-1"),
	);

	assert!(!dispatch.response.success);
	assert!(matches!(
		dispatch.response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("invalid response with no `contentItems`")
	));
	assert_eq!(
		dispatch
			.terminal_failure
			.as_ref()
			.map(super::super::AppServerDynamicToolFailure::error_class),
		Some("app_server_dynamic_tool_protocol_failure")
	);
}

#[test]
fn dynamic_tool_call_records_tool_failures_without_terminal_protocol_failure() {
	let handler = FailingToolHandler;
	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"threadId": "thread-1",
			"tool": "failing_tool",
			"turnId": "turn-1"
		}),
	};
	let dispatch = super::super::handle_dynamic_tool_call(
		Some(&handler),
		&request,
		"thread-1",
		Some("turn-1"),
	);

	assert!(!dispatch.response.success);
	assert!(dispatch.terminal_failure.is_none());

	let diagnostic = dispatch.diagnostic.expect("tool failure should publish a diagnostic");

	assert_eq!(diagnostic.failure_class, "app_server_dynamic_tool_failed");
	assert_eq!(diagnostic.tool.as_deref(), Some("failing_tool"));
	assert_eq!(diagnostic.message, "tool rejected the request");
}

#[test]
fn dynamic_tool_call_can_validate_thread_without_fixed_turn_during_steer_rpc() {
	let handler = NamespacedDynamicToolHandler { seen_namespace: RefCell::new(None) };
	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"namespace": "tracker",
			"threadId": "thread-1",
			"tool": "tracker_tool",
			"turnId": "turn-after-steer"
		}),
	};
	let dispatch =
		super::super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", None);

	assert!(dispatch.response.success);
	assert!(dispatch.terminal_failure.is_none());
	assert_eq!(*handler.seen_namespace.borrow(), Some(String::from("tracker")));
}

#[test]
fn usage_limit_notification_stops_current_turn_without_operator_attention() {
	let notification = JsonRpcNotification {
		method: String::from("error"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-1",
			"willRetry": false,
			"error": {
				"message": "You've hit your usage limit.",
				"codexErrorInfo": "usageLimitExceeded"
			}
		}),
	};
	let mut final_output = String::new();
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;
	let error = super::super::handle_turn_execution_notification(
		&notification,
		"thread-1",
		"turn-1",
		&mut final_output,
		&mut latest_turn_failure,
	);
	let Err(error) = error else {
		panic!("usage limit should fail the current turn immediately");
	};
	let failure =
		error.downcast_ref::<AppServerTurnFailure>().expect("error should be a turn failure");

	assert_eq!(failure.error_class(), "app_server_usage_limit_exceeded");
	assert!(failure.is_retryable_capacity_failure());
	assert!(!failure.requires_operator_attention());
	assert!(latest_turn_failure.is_none());
}
