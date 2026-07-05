use serde_json::{self};

use crate::agent::app_server::{self, UserInput};

#[test]
fn turn_start_request_uses_default_runtime_settings() {
	let request = app_server::build_turn_start_request("thread-1", "hello");

	assert_eq!(request.thread_id, "thread-1");
	assert!(matches!(
		request.input.as_slice(),
		[UserInput::Text{ text }] if text == "hello"
	));
}

#[test]
fn turn_steer_request_uses_expected_turn_precondition_and_text() {
	let request = app_server::build_turn_steer_request("thread-1", "turn-1", "change direction");

	assert_eq!(request.thread_id, "thread-1");
	assert_eq!(request.expected_turn_id, "turn-1");
	assert!(matches!(
		request.input.as_slice(),
		[UserInput::Text{ text }] if text == "change direction"
	));
}

#[test]
fn turn_start_request_omits_execution_policy_overrides() {
	let request = app_server::build_turn_start_request("thread-1", "hello");
	let value = serde_json::to_value(&request).expect("turn start request should serialize");

	assert_eq!(value["threadId"], "thread-1");
	assert!(value.get("model").is_none());
	assert!(value.get("modelProvider").is_none());
	assert!(value.get("personality").is_none());
	assert!(value.get("serviceTier").is_none());
	assert!(value.get("approvalPolicy").is_none());
	assert!(value.get("sandboxPolicy").is_none());
	assert!(value.get("config").is_none());
}
