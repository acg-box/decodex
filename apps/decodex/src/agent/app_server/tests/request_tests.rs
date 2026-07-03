use std::{env, fs, time::Duration};

use serde_json::{self, Value};
use tempfile::TempDir;

use crate::{
	agent::{
		app_server::{
			self, AppServerDynamicToolFailure, CommandExecHealthCheck, UserInput, tests,
			tests::InvalidToolNameHandler,
		},
		tracker_tool_bridge::{DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec},
	},
	state::{self, StateStore},
	test_support::TestEnvVarGuard,
};

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

#[test]
fn synthetic_probe_thread_start_is_ephemeral_when_requested() {
	let mut request = tests::minimal_run_request();

	request.ephemeral_thread = true;

	let start = app_server::build_thread_start_request(&request).expect("request should build");
	let value = serde_json::to_value(&start).expect("thread start request should serialize");

	assert_eq!(value["ephemeral"], true);
}

#[test]
fn thread_session_timeout_allows_slow_app_server_setup() {
	assert!(app_server::THREAD_SESSION_REQUEST_TIMEOUT > app_server::REQUEST_TIMEOUT);
	assert_eq!(app_server::THREAD_SESSION_REQUEST_TIMEOUT, Duration::from_secs(30));
}

#[test]
fn app_server_run_accepts_thread_start_after_base_request_timeout() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let fake_bin_dir =
		tests::install_fake_codex_script(&temp_dir, &tests::slow_thread_start_fake_codex_script());
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("slow-thread-start-run");
	request.issue_id = String::from("slow-thread-start-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(20);

	let result = app_server::execute_app_server_run(&request, &state_store)
		.expect("thread/start slower than base request timeout should still complete");

	assert_eq!(result.thread_id, "thread-1");
	assert_eq!(result.turn_id, "turn-1");
	assert_eq!(result.final_output, "ORPHAN_OK");
}

#[test]
fn command_exec_health_check_uses_bounded_standalone_request() {
	let health_check = CommandExecHealthCheck {
		command: vec![String::from("/bin/sh"), String::from("-c"), String::from("printf ok")],
		expected_stdout: String::from("ok"),
		timeout_ms: 1_000,
		output_bytes_cap: 128,
	};
	let params = app_server::build_command_exec_health_check_params(&health_check, "/tmp/worktree");
	let value = serde_json::to_value(&params).expect("command exec params should serialize");

	assert_eq!(value["command"], serde_json::json!(["/bin/sh", "-c", "printf ok"]));
	assert_eq!(value["cwd"], "/tmp/worktree");
	assert_eq!(value["timeoutMs"], 1_000);
	assert_eq!(value["outputBytesCap"], 128);
	assert!(value.get("threadId").is_none());
	assert!(value.get("sandboxPolicy").is_none());
	assert!(value.get("permissionProfile").is_none());
}

#[test]
fn turn_completion_ignores_orphan_json_rpc_response() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let marker_path = temp_dir.path().join("activity");
	let fake_bin_dir =
		tests::install_fake_codex_script(&temp_dir, tests::orphan_response_fake_codex_script());
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("orphan-response-run");
	request.issue_id = String::from("orphan-response-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);
	request.activity_marker_path = Some(marker_path.clone());

	let result = app_server::execute_app_server_run(&request, &state_store)
		.expect("orphan response during turn wait should not fail the run");

	assert_eq!(result.thread_id, "thread-1");
	assert_eq!(result.turn_id, "turn-1");
	assert_eq!(result.final_output, "ORPHAN_OK");

	let marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let protocol_activity =
		marker.protocol_activity().expect("protocol activity should be captured");

	assert!(state_store.event_count(&request.run_id).expect("event count should load") > 0);
	assert!(
		protocol_activity.recent_events.iter().any(|event| event.event_type == "json-rpc/response")
	);
	assert_eq!(marker.last_event_type(), Some("turn/completed"));
}

#[test]
fn turn_completion_waits_through_retrying_error_notification() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let marker_path = temp_dir.path().join("activity");
	let fake_bin_dir =
		tests::install_fake_codex_script(&temp_dir, &tests::retrying_error_fake_codex_script());
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("retrying-error-run");
	request.issue_id = String::from("retrying-error-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);
	request.activity_marker_path = Some(marker_path.clone());

	let result = app_server::execute_app_server_run(&request, &state_store)
		.expect("retrying error during turn wait should not fail the run");

	assert_eq!(result.thread_id, "thread-1");
	assert_eq!(result.turn_id, "turn-1");
	assert_eq!(result.final_output, "ORPHAN_OK");

	let marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_event_type(), Some("turn/completed"));
	assert!(
		state_store
			.run_has_protocol_event(&request.run_id, "error")
			.expect("retrying error event lookup should load")
	);
}
