use serde_json::{Value, json};

use decodex_codex::app_server_client::ServerEvent;

use super::{
	control_turn_params, default_controller_model, event_failure, native_metadata, tool_text,
};
use crate::ControlOrigin;

#[test]
fn application_control_retains_tool_authority() {
	let params = control_turn_params("isolated", "Question", ControlOrigin::Application);
	assert_eq!(params["turnTrigger"], "goal");
	assert_eq!(params["input"], json!([]));
	assert_eq!(params["toolOutput"]["namespace"], "decodex");
	assert_eq!(params["toolOutput"]["output"], "Question");
	let direct = control_turn_params("isolated", "Question", ControlOrigin::User);
	assert_eq!(direct["input"][0]["text"], "Question");
	assert!(direct.get("toolOutput").is_none());
}

fn completed(metadata: Value) -> Value {
	json!({"item":{"type":"mcpToolCall","server":"node_repl","result":{"content":[{
		"type":"text","text":json!({"kind":"decodex_pro_native_context","value":metadata}).to_string()
	}]}}})
}

#[test]
fn native_context_preserves_all_approval_fields_and_rejects_foreign_turns() {
	let metadata = json!({"session_id":"real-thread","thread_id":"real-thread","turn_id":"real-turn",
		"node_repl_auto_review_required":true,"auto_review_enabled":false,"model":"official-model",
		"future_native_field":{"preserved":true}});
	for value in [metadata.clone(), Value::String(metadata.to_string())] {
		assert_eq!(
			native_metadata(&completed(value.clone()), "node_repl", "real-thread", "real-turn")
				.expect("verified"),
			Some(value)
		);
	}
	assert!(
		native_metadata(&completed(metadata), "node_repl", "real-thread", "different-turn")
			.is_err()
	);
	assert!(
		native_metadata(
			&completed(
				json!({"session_id":"real-thread","thread_id":"real-thread","turn_id":"real-turn"})
			),
			"node_repl",
			"real-thread",
			"real-turn"
		)
		.is_err()
	);
}

#[test]
fn an_official_review_rejection_is_not_converted_to_a_success_or_retried() {
	let result =
		json!({"isError":true,"content":[{"type":"text","text":"Explicit approval required."}]});
	assert!(tool_text(&result).expect_err("rejected").contains("Explicit approval required"));
}

#[test]
fn native_failures_keep_the_reported_reason_and_ignore_other_threads() {
	for (method, params) in [
		(
			"error",
			json!({"threadId":"owned","willRetry":false,"error":{"message":"Native account is unavailable"}}),
		),
		(
			"turn/completed",
			json!({"threadId":"owned","turn":{"id":"turn","error":{"message":"Native account is unavailable"}}}),
		),
	] {
		let event = ServerEvent::Notification { method: method.into(), params };
		assert!(
			event_failure(&event, Some("owned"), Some("turn"))
				.expect("native failure")
				.contains("Native account is unavailable")
		);
		assert!(event_failure(&event, Some("different"), Some("turn")).is_none());
	}
}

#[test]
fn controller_uses_the_native_default_instead_of_a_display_name_or_shared_config_alias() {
	let catalog = json!({"data":[
		{"id":"first","model":"other-native-model","displayName":"GPT Something","isDefault":false},
		{"id":"default","model":"native-controller","displayName":"GPT Default","isDefault":true}
	]});
	assert_eq!(default_controller_model(&catalog).expect("native default"), "native-controller");
	assert!(default_controller_model(&json!({"data":[]})).is_err());
}
