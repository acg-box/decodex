use crate::{
	agent::{
		app_server::tests::{AppServerTurnFailure, JsonRpcError, JsonRpcErrorPayload, runtime},
		json_rpc::JsonRpcNotification,
	},
	prelude::eyre,
};

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
	let result = runtime::handle_turn_execution_notification(
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
	let result = runtime::handle_turn_execution_notification(
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
	let failure = runtime::turn_failure_from_json_rpc_error_response("thread-1", "turn-1", &error);
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
	let failure_class = runtime::steer_error_class(&error);

	assert_eq!(failure_class, "active_turn_not_steerable");
}

#[test]
fn steer_delivery_error_classifies_missing_method_as_unsupported() {
	for message in [
		"`turn/steer` failed with -32601: method not found",
		"`turn/steer` failed with -32601: Method not found",
	] {
		let error = eyre::eyre!("{message}");
		let failure_class = runtime::steer_error_class(&error);

		assert_eq!(failure_class, "app_server_turn_steer_unsupported");
	}
}
