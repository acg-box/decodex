use crate::agent::{
	app_server::tests::{AppServerTurnFailure, runtime},
	json_rpc::JsonRpcNotification,
};

#[test]
fn app_server_turn_failure_classifies_operator_attention() {
	for (case_name, message, code, requires_attention, error_class) in [
		(
			"usage limit",
			"You've hit your usage limit.",
			Some(String::from("usageLimitExceeded")),
			false,
			"app_server_usage_limit_exceeded",
		),
		("generic failure", "transient model failure", None, false, "app_server_turn_failed"),
	] {
		let failure = AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			message,
			code.clone(),
		);

		assert_eq!(failure.requires_operator_attention(), requires_attention, "{case_name}");
		assert_eq!(failure.error_class(), error_class);
		assert_eq!(failure.should_stop_current_turn(), case_name == "usage limit", "{case_name}");

		if let Some(code) = code {
			assert!(failure.to_string().contains(&code));
		}
	}
}

#[test]
fn structured_error_notification_becomes_turn_failure() {
	let notification = JsonRpcNotification {
		method: String::from("error"),
		params: serde_json::json!({
			"error": {
				"message": {
					"kind": "protocolFailure",
					"detail": "unexpected response"
				},
				"codexErrorInfo": {
					"type": "appServerProtocolMismatch"
				}
			},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"willRetry": false
		}),
	};
	let (failure, will_retry) =
		runtime::failure_from_error_notification(&notification, "thread-1", "turn-1")
			.expect("structured error payload should parse")
			.expect("matching error notification should produce a failure");
	let failure_message = failure.to_string();

	assert!(failure_message.contains("protocolFailure"));
	assert!(failure_message.contains("appServerProtocolMismatch"));
	assert_eq!(will_retry, Some(false));
}

#[test]
fn retrying_error_notification_does_not_replace_latest_turn_failure() {
	let notification = JsonRpcNotification {
		method: String::from("error"),
		params: serde_json::json!({
			"error": {
				"message": "Reconnecting... 2/5",
				"codexErrorInfo": "transientNetworkError"
			},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"willRetry": true
		}),
	};
	let mut final_output = String::new();
	let mut latest_turn_failure = Some(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"previous transient failure",
		None,
	));
	let outcome = runtime::handle_turn_execution_notification(
		&notification,
		"thread-1",
		"turn-1",
		&mut final_output,
		&mut latest_turn_failure,
	)
	.expect("retrying error notification should remain nonterminal");

	assert!(outcome.is_none());
	assert_eq!(
		latest_turn_failure,
		Some(AppServerTurnFailure::new(
			"thread-1",
			Some(String::from("turn-1")),
			"failed",
			"previous transient failure",
			None,
		))
	);
}

#[test]
fn thread_system_error_notification_fails_turn_immediately() {
	let notification = JsonRpcNotification {
		method: String::from("thread/status/changed"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"status": {
				"type": "systemError",
				"activeFlags": []
			}
		}),
	};
	let mut final_output = String::new();
	let mut latest_turn_failure = None;
	let result = runtime::handle_turn_execution_notification(
		&notification,
		"thread-1",
		"turn-1",
		&mut final_output,
		&mut latest_turn_failure,
	);
	let error = match result {
		Ok(_) => panic!("systemError should fail the turn immediately"),
		Err(error) => error,
	};
	let failure =
		error.downcast_ref::<AppServerTurnFailure>().expect("error should be a turn failure");

	assert!(failure.to_string().contains("systemError"));
	assert_eq!(failure.error_class(), "app_server_turn_failed");
	assert!(latest_turn_failure.is_none());
}
