use serde_json::Value;

use crate::agent::{
	decodex_tool_bridge::{
		DECODEX_RUN_CONTEXT_NAMESPACE, DECODEX_RUN_CONTEXT_TOOL_NAME, DecodexRunContext,
		DecodexToolBridge,
	},
	tracker_tool_bridge::{
		DynamicToolCallResponse, DynamicToolContentItem, DynamicToolHandler, DynamicToolSpec,
	},
};

struct FakeTrackerTools;
impl DynamicToolHandler for FakeTrackerTools {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"issue_comment",
			"Add a comment.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::success(format!("delegated {tool_name}"))
	}
}

fn sample_bridge() -> DecodexToolBridge<'static> {
	DecodexToolBridge::new(
		&FakeTrackerTools,
		DecodexRunContext {
			run_id: String::from("run-1"),
			attempt_number: 2,
			issue_id: String::from("issue-1"),
			issue_identifier: String::from("XY-449"),
			branch: String::from("y/decodex-xy-449"),
			worktree_path: String::from("/tmp/worktree"),
			max_turns: 3,
			default_canonicalize_commands: vec![String::from("cargo make fmt")],
			default_verify_commands: vec![String::from("cargo make test")],
		},
	)
}

#[test]
fn publishes_deferred_run_context_tool_with_protocol_safe_name() {
	let specs = sample_bridge().tool_specs();
	let run_context = specs
		.iter()
		.find(|spec| spec.name == DECODEX_RUN_CONTEXT_TOOL_NAME)
		.expect("run context tool should be published");
	let protocol_identifier_is_safe = |name: &str| {
		!name.is_empty()
			&& name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
	};

	assert!(run_context.defer_loading);
	assert_eq!(run_context.namespace.as_deref(), Some(DECODEX_RUN_CONTEXT_NAMESPACE));
	assert!(protocol_identifier_is_safe(&run_context.name));
	assert!(protocol_identifier_is_safe(
		run_context.namespace.as_deref().expect("run context tool should be namespaced")
	));
	assert!(specs.iter().all(|spec| protocol_identifier_is_safe(&spec.name)));
	assert!(
		specs.iter().filter_map(|spec| spec.namespace.as_deref()).all(protocol_identifier_is_safe)
	);
}

#[test]
fn returns_run_context_json() {
	let response = sample_bridge().handle_call_with_namespace(
		Some(DECODEX_RUN_CONTEXT_NAMESPACE),
		DECODEX_RUN_CONTEXT_TOOL_NAME,
		serde_json::json!({}),
	);

	assert!(response.success);

	let [DynamicToolContentItem::InputText { text }] = response.content_items.as_slice() else {
		panic!("run context should return one text item");
	};
	let value: Value = serde_json::from_str(text).expect("run context should be JSON");

	assert_eq!(value["runId"], "run-1");
	assert_eq!(value["issueIdentifier"], "XY-449");
	assert_eq!(value["defaultVerifyCommands"][0], "cargo make test");
}

#[test]
fn validates_run_context_arguments() {
	let response = sample_bridge().handle_call_with_namespace(
		Some(DECODEX_RUN_CONTEXT_NAMESPACE),
		DECODEX_RUN_CONTEXT_TOOL_NAME,
		serde_json::json!({ "unexpected": true }),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Invalid `decodex_run_context` arguments")
	));
}

#[test]
fn delegates_tracker_tools() {
	let response = sample_bridge().handle_call("issue_comment", serde_json::json!({}));

	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText { text: String::from("delegated issue_comment") }]
	);
}
