use crate::{
	agent::{
		app_server::tests::{AppServerTurnFailure, JsonRpcError, JsonRpcErrorPayload},
		json_rpc::JsonRpcNotification,
	},
	prelude::eyre,
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
		super::failure_from_error_notification(&notification, "thread-1", "turn-1")
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
	let outcome = super::handle_turn_execution_notification(
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
	let result = super::handle_turn_execution_notification(
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

#[test]
fn thread_system_error_notification_fails_immediately_with_latest_turn_failure() {
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
	let mut latest_turn_failure = Some(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"previous structured turn error",
		None,
	));
	let result = super::handle_turn_execution_notification(
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

	assert!(failure.to_string().contains("previous structured turn error"));
	assert_eq!(failure.error_class(), "app_server_turn_failed");
	assert!(latest_turn_failure.is_none());
}

#[test]
fn turn_completed_without_error_payload_becomes_structured_turn_failure() {
	let notification = JsonRpcNotification {
		method: String::from("turn/completed"),
		params: serde_json::json!({
			"turn": {
				"id": "turn-1",
				"status": "interrupted",
				"error": null
			}
		}),
	};
	let mut final_output = String::new();
	let mut latest_turn_failure = None;
	let result = super::handle_turn_execution_notification(
		&notification,
		"thread-1",
		"turn-1",
		&mut final_output,
		&mut latest_turn_failure,
	);
	let error = match result {
		Ok(_) => panic!("interrupted turn without payload should fail the turn"),
		Err(error) => error,
	};
	let failure =
		error.downcast_ref::<AppServerTurnFailure>().expect("error should be a turn failure");

	assert_eq!(failure.error_class(), "app_server_turn_missing_error_payload");
	assert!(failure.to_string().contains("without an explicit error payload"));
	assert!(latest_turn_failure.is_none());
}

#[test]
fn missing_error_payload_terminal_guidance_is_status_neutral() {
	let notification = JsonRpcNotification {
		method: String::from("turn/completed"),
		params: serde_json::json!({
			"turn": {
				"id": "turn-1",
				"status": "failed",
				"error": null
			}
		}),
	};
	let mut final_output = String::new();
	let mut latest_turn_failure = None;
	let result = super::handle_turn_execution_notification(
		&notification,
		"thread-1",
		"turn-1",
		&mut final_output,
		&mut latest_turn_failure,
	);
	let error = match result {
		Ok(_) => panic!("failed turn without payload should fail the turn"),
		Err(error) => error,
	};
	let failure =
		error.downcast_ref::<AppServerTurnFailure>().expect("error should be a turn failure");
	let next_action = failure.terminal_next_action("recover manually");

	assert!(next_action.contains("terminal turn status `failed`"));
	assert!(next_action.contains("terminal turn left useful worktree changes"));
	assert!(!next_action.contains("interrupted turn"));
}

#[test]
fn json_rpc_error_response_becomes_recoverable_turn_failure() {
	let error = JsonRpcError {
		id: serde_json::json!(7),
		error: JsonRpcErrorPayload {
			code: -32_000,
			message: String::from("late response"),
			data: None,
		},
	};
	let failure = super::turn_failure_from_json_rpc_error_response("thread-1", "turn-1", &error);
	let failure_message = failure.to_string();

	assert!(failure_message.contains("thread-1"));
	assert!(failure_message.contains("turn-1"));
	assert!(failure_message.contains("code -32000"));
	assert!(failure_message.contains("late response"));
}

#[test]
fn steer_delivery_error_classifies_active_turn_not_steerable_distinctly() {
	let error = eyre::eyre!(
		"`turn/steer` failed with -32000: turn is not steerable data: {{\"type\":\"activeTurnNotSteerable\"}}"
	);
	let failure_class = super::steer_error_class(&error);

	assert_eq!(failure_class, "active_turn_not_steerable");
}

#[test]
fn steer_delivery_error_classifies_missing_method_as_unsupported() {
	for message in [
		"`turn/steer` failed with -32601: method not found",
		"`turn/steer` failed with -32601: Method not found",
	] {
		let error = eyre::eyre!("{message}");
		let failure_class = super::steer_error_class(&error);

		assert_eq!(failure_class, "app_server_turn_steer_unsupported");
	}
}
