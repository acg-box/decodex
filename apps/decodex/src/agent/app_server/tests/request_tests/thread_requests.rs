use serde_json::{self, Value};

use crate::agent::{
	app_server::{self, AppServerDynamicToolFailure, tests, tests::InvalidToolNameHandler},
	tracker_tool_bridge::{DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec},
};

#[test]
fn thread_start_and_resume_requests_inherit_runtime_config() {
	fn assert_runtime_config(value: &Value) {
		assert_eq!(value["cwd"], "/tmp/worktree");
		assert_eq!(value["developerInstructions"], "Follow the workflow.");
		assert!(value.get("model").is_none());
		assert!(value.get("modelProvider").is_none());
		assert!(value.get("personality").is_none());
		assert!(value.get("serviceTier").is_none());
		assert!(value.get("approvalPolicy").is_none());
		assert!(value.get("sandbox").is_none());
		assert!(value.get("config").is_none());
		assert!(value.get("ephemeral").is_none());
	}

	let start = app_server::build_thread_start_request(&tests::minimal_run_request())
		.expect("request should build");
	let start_value = serde_json::to_value(&start).expect("thread start request should serialize");
	let resume = app_server::build_thread_resume_request("thread-1", &tests::minimal_run_request());
	let resume_value =
		serde_json::to_value(&resume).expect("thread resume request should serialize");

	assert_runtime_config(&start_value);
	assert_runtime_config(&resume_value);

	assert_eq!(resume_value["threadId"], "thread-1");
}

#[test]
fn thread_start_serializes_dynamic_tools_with_app_server_141_shape() {
	struct MixedDynamicToolHandler;

	impl DynamicToolHandler for MixedDynamicToolHandler {
		fn tool_specs(&self) -> Vec<DynamicToolSpec> {
			let local_tool = DynamicToolSpec::new(
				"local_tool",
				"Test local tool.",
				serde_json::json!({
					"type": "object",
					"additionalProperties": false
				}),
			);
			let mut tracker_tool = DynamicToolSpec::new(
				"tracker_tool",
				"Test namespaced tool.",
				serde_json::json!({
					"type": "object",
					"additionalProperties": false
				}),
			)
			.deferred();

			tracker_tool.namespace = Some(String::from("tracker"));

			vec![local_tool, tracker_tool]
		}

		fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
			DynamicToolCallResponse::success(String::from("unused"))
		}
	}

	let handler = MixedDynamicToolHandler;
	let mut request = tests::minimal_run_request();

	request.dynamic_tool_handler = Some(&handler);

	let start = app_server::build_thread_start_request(&request).expect("request should build");
	let start_value = serde_json::to_value(&start).expect("thread start request should serialize");
	let dynamic_tools =
		start_value["dynamicTools"].as_array().expect("dynamicTools should serialize as an array");

	assert_eq!(dynamic_tools.len(), 2);
	assert_eq!(dynamic_tools[0]["type"], "function");
	assert_eq!(dynamic_tools[0]["name"], "local_tool");
	assert_eq!(dynamic_tools[0]["description"], "Test local tool.");
	assert!(dynamic_tools[0].get("namespace").is_none());
	assert_eq!(dynamic_tools[1]["type"], "namespace");
	assert_eq!(dynamic_tools[1]["name"], "tracker");
	assert_eq!(dynamic_tools[1]["description"], "Dynamic tools in the tracker namespace.");
	assert!(dynamic_tools[1].get("inputSchema").is_none());
	assert!(dynamic_tools[1].get("namespace").is_none());

	let namespace_tools =
		dynamic_tools[1]["tools"].as_array().expect("namespace tool should contain tools array");

	assert_eq!(namespace_tools.len(), 1);
	assert_eq!(namespace_tools[0]["type"], "function");
	assert_eq!(namespace_tools[0]["name"], "tracker_tool");
	assert_eq!(namespace_tools[0]["description"], "Test namespaced tool.");
	assert_eq!(namespace_tools[0]["deferLoading"], true);
	assert!(namespace_tools[0].get("namespace").is_none());
}

#[test]
fn thread_start_rejects_invalid_dynamic_tool_names() {
	let handler = InvalidToolNameHandler;
	let mut request = tests::minimal_run_request();

	request.dynamic_tool_handler = Some(&handler);

	let error = app_server::build_thread_start_request(&request)
		.expect_err("invalid dynamic tool name should fail before thread/start");
	let failure = error
		.downcast_ref::<AppServerDynamicToolFailure>()
		.expect("invalid dynamic tool should classify as a dynamic tool failure");

	assert_eq!(failure.error_class(), "app_server_dynamic_tool_protocol_failure");
	assert!(error.to_string().contains("identifier pattern"));
}

#[test]
fn synthetic_probe_thread_start_is_ephemeral_when_requested() {
	let mut request = tests::minimal_run_request();

	request.ephemeral_thread = true;

	let start = app_server::build_thread_start_request(&request).expect("request should build");
	let value = serde_json::to_value(&start).expect("thread start request should serialize");

	assert_eq!(value["ephemeral"], true);
}
