use std::{
	cell::RefCell,
	collections::BTreeMap,
	env, fs,
	os::unix::fs::PermissionsExt,
	path::PathBuf,
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde_json::{self, Value};
use tempfile::TempDir;

use crate::{
	agent::{
		app_server::{
			APP_SERVER_SCHEMA_REQUIRED_MARKERS, AppServerCapabilityPreflightFailure,
			AppServerCapabilityPreflightReport, AppServerDynamicToolFailure,
			AppServerPhaseGoalFailure, AppServerRunResult, AppServerThreadArchiveOutcome,
			AppServerThreadArchiveRequest, AppServerTurnFailure, CommandExecHealthCheck,
			CommandExecResponse, EffectiveThreadConfig, InitializeResponse,
			ModelProviderCapabilitiesReadResponse, PhaseGoalController, PhaseGoalKind,
			PhaseGoalSpec, PhaseGoalTransition, PluginListResponse, ProbeDynamicToolHandler,
			REQUEST_TIMEOUT, RequestWaitPhase, RunRecorder, RuntimeConfigSummary,
			SkillsListResponse, TurnContinuationGuard, UserInput,
		},
		json_rpc::{
			AppServerHomePreflightFailure, AppServerOutputTimeout, AppServerProcessEnv,
			JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
			ResolvedAppServerCodexHomeEnv, WireMessage,
		},
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolContentItem, DynamicToolHandler, DynamicToolSpec,
			TurnCompletionStatus,
		},
	},
	prelude::{Result, eyre},
	run_control::{
		self, LaneControlSteerRequest, LaneControlSteerRequestInput, LaneControlSteerResponse,
	},
	state::{self, ProtocolActivitySummary, StateStore},
	test_support::TestEnvVarGuard,
};

const PHASE_GOAL_FAKE_CODEX_SCRIPT_TEMPLATE: &str = r#"#!/usr/bin/env python3
import json
import os
import sys

TURN_OUTPUTS = __TURN_OUTPUTS__
GOAL_STATUSES = __GOAL_STATUSES__
UNSUPPORTED_GOAL_METHODS = __UNSUPPORTED_GOAL_METHODS__

goal = {
    "objective": "",
    "tokenBudget": None,
}
turn_count = 0
goal_get_count = 0

def send(value):
    print(json.dumps(value), flush=True)

def reply(message_id, result):
    send({"id": message_id, "result": result})

def method_not_found(message_id, method):
    send({"id": message_id, "error": {
        "code": -32601,
        "message": "Method not found: " + str(method),
    }})

def unsupported_goal_method(method):
    return method in UNSUPPORTED_GOAL_METHODS

def goal_payload(status):
    return {
        "createdAt": 1,
        "objective": goal["objective"],
        "status": status,
        "threadId": "thread-1",
        "timeUsedSeconds": 0,
        "tokenBudget": goal["tokenBudget"],
        "tokensUsed": 0,
        "updatedAt": 1,
    }

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params") or {}

    if method == "initialize":
        reply(message_id, {
            "userAgent": "codex-cli 0.136.0",
            "codexHome": os.environ["CODEX_HOME"],
            "platformFamily": "unix",
            "platformOs": "macos",
        })
    elif method == "initialized":
        continue
    elif method == "config/read":
        reply(message_id, {"config": {
            "model": "gpt-5.5",
            "model_provider": "openai",
            "approval_policy": {"type": "never"},
            "sandbox_mode": {"type": "dangerFullAccess"},
        }})
    elif method == "model/list":
        reply(message_id, {"data": [{
            "id": "gpt-5.5",
            "model": "gpt-5.5",
            "displayName": "GPT-5.5",
            "isDefault": True,
            "hidden": False,
        }], "nextCursor": None})
    elif method == "modelProvider/capabilities/read":
        reply(message_id, {"imageGeneration": True, "namespaceTools": True, "webSearch": True})
    elif method == "skills/list":
        cwd = params.get("cwds", [""])[0]
        reply(message_id, {"data": [{"cwd": cwd, "errors": [], "skills": [{
            "enabled": True,
            "name": "fake-skill",
            "scope": "user",
        }]}]})
    elif method == "plugin/list":
        reply(message_id, {"marketplaces": [{"name": "fake", "plugins": [{
            "enabled": True,
            "id": "fake-plugin",
            "installed": True,
            "name": "Fake Plugin",
        }]}], "marketplaceLoadErrors": []})
    elif method == "mcpServerStatus/list":
        reply(message_id, {"data": [], "nextCursor": None})
    elif method == "thread/start":
        reply(message_id, {
            "thread": {"id": "thread-1"},
            "model": "gpt-5.5",
            "modelProvider": "openai",
            "serviceTier": None,
            "cwd": params.get("cwd"),
            "instructionSources": [],
            "approvalPolicy": {"type": "never"},
            "approvalsReviewer": "user",
            "sandbox": {"type": "dangerFullAccess"},
            "reasoningEffort": None,
        })
    elif method == "thread/goal/set":
        if unsupported_goal_method(method):
            method_not_found(message_id, method)
        else:
            goal["objective"] = params.get("objective") or ""
            goal["tokenBudget"] = params.get("tokenBudget")
            reply(message_id, {"goal": goal_payload("active")})
    elif method == "thread/goal/get":
        if unsupported_goal_method(method):
            method_not_found(message_id, method)
        else:
            if GOAL_STATUSES:
                status = GOAL_STATUSES[min(goal_get_count, len(GOAL_STATUSES) - 1)]
            else:
                status = "active"
            goal_get_count += 1
            reply(message_id, {"goal": None if status == "none" else goal_payload(status)})
    elif method == "thread/goal/clear":
        if unsupported_goal_method(method):
            method_not_found(message_id, method)
        else:
            reply(message_id, {"cleared": True})
    elif method == "turn/start":
        turn_count += 1
        turn_id = "turn-" + str(turn_count)
        if TURN_OUTPUTS:
            output = TURN_OUTPUTS[min(turn_count - 1, len(TURN_OUTPUTS) - 1)]
        else:
            output = "DONE"
        reply(message_id, {"turn": {"id": turn_id, "status": "running", "error": None}})
        send({"method": "thread/status/changed", "params": {
            "threadId": "thread-1",
            "status": {"type": "active", "activeFlags": []},
        }})
        send({"method": "turn/started", "params": {
            "threadId": "thread-1",
            "turn": {"id": turn_id, "status": "running", "error": None},
        }})
        if not unsupported_goal_method("thread/goal/updated"):
            send({"method": "thread/goal/updated", "params": {
                "threadId": "thread-1",
                "turnId": turn_id,
                "goal": goal_payload("active"),
            }})
        send({"method": "item/completed", "params": {
            "threadId": "thread-1",
            "turnId": turn_id,
            "item": {"type": "agentMessage", "text": output},
        }})
        send({"method": "turn/completed", "params": {
            "threadId": "thread-1",
            "turn": {"id": turn_id, "status": "completed", "error": None},
        }})
    else:
        method_not_found(message_id, method)
"#;

struct RejectingCompletionHandler;
impl DynamicToolHandler for RejectingCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn validate_turn_completion(&self, _final_output: &str) -> Result<()> {
		Err(eyre::eyre!("terminal finalization missing"))
	}
}

struct ContinuingCompletionHandler;
impl DynamicToolHandler for ContinuingCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn classify_turn_completion(&self, _final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(TurnCompletionStatus::Continue)
	}

	fn validate_turn_completion(&self, _final_output: &str) -> Result<()> {
		Err(eyre::eyre!("terminal finalization missing"))
	}
}

struct InvalidToolNameHandler;
impl DynamicToolHandler for InvalidToolNameHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"invalid.tool",
			"Invalid test tool.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::success(String::from("unused"))
	}
}

struct EmptyToolResponseHandler;
impl DynamicToolHandler for EmptyToolResponseHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"empty_response",
			"Return an invalid empty response.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse { content_items: Vec::new(), success: true }
	}
}

struct FailingToolHandler;
impl DynamicToolHandler for FailingToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"failing_tool",
			"Return a normal tool failure response.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("tool rejected the request"))
	}
}

struct YieldingContinuationGuard;
impl TurnContinuationGuard for YieldingContinuationGuard {
	fn should_continue_turn(&self, _turn_count: u32) -> Result<bool> {
		Ok(false)
	}
}

struct RejectingContinuationGuard;
impl TurnContinuationGuard for RejectingContinuationGuard {
	fn should_continue_turn(&self, _turn_count: u32) -> Result<bool> {
		Ok(false)
	}

	fn validate_continuation_boundary(&self, turn_count: u32) -> Result<()> {
		Err(eyre::eyre!("turn {turn_count} hit an invalid continuation boundary"))
	}
}

struct NamespacedDynamicToolHandler {
	seen_namespace: RefCell<Option<String>>,
}
impl DynamicToolHandler for NamespacedDynamicToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut spec = DynamicToolSpec::new(
			"tracker_tool",
			"Test namespaced tool.",
			serde_json::json!({
				"type": "object",
				"additionalProperties": false
			}),
		);

		spec.namespace = Some(String::from("tracker"));

		vec![spec]
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("namespace should be forwarded"))
	}

	fn handle_call_with_namespace(
		&self,
		namespace: Option<&str>,
		_tool_name: &str,
		_arguments: Value,
	) -> DynamicToolCallResponse {
		self.seen_namespace.replace(namespace.map(str::to_owned));

		DynamicToolCallResponse::success(String::from("ok"))
	}
}

struct LiveResumeDynamicToolHandler;
impl DynamicToolHandler for LiveResumeDynamicToolHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		vec![DynamicToolSpec::new(
			"echo_resume",
			"Echo the provided integration text.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"text": { "type": "string" }
				},
				"required": ["text"],
				"additionalProperties": false
			}),
		)]
	}

	fn handle_call(&self, tool_name: &str, arguments: Value) -> DynamicToolCallResponse {
		if tool_name != "echo_resume" {
			return DynamicToolCallResponse::failure(format!(
				"Unexpected live integration tool `{tool_name}`."
			));
		}

		let Some(text) = arguments.get("text").and_then(Value::as_str) else {
			return DynamicToolCallResponse::failure(String::from(
				"`echo_resume` requires a string `text` argument.",
			));
		};

		DynamicToolCallResponse::success(text.to_owned())
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(match final_output.trim() {
			"CONTINUE" => TurnCompletionStatus::Continue,
			_ => TurnCompletionStatus::Complete,
		})
	}
}

struct LiveResumeBoundaryGuard;
impl TurnContinuationGuard for LiveResumeBoundaryGuard {
	fn should_continue_turn(&self, _turn_count: u32) -> Result<bool> {
		Ok(false)
	}
}

struct ContinueTokenCompletionHandler;
impl DynamicToolHandler for ContinueTokenCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(if final_output.trim() == "CONTINUE" {
			TurnCompletionStatus::Continue
		} else {
			TurnCompletionStatus::Complete
		})
	}
}

#[derive(Default)]
struct TerminalTokenCompletionHandler {
	terminal_seen: RefCell<bool>,
}
impl DynamicToolHandler for TerminalTokenCompletionHandler {
	fn tool_specs(&self) -> Vec<DynamicToolSpec> {
		Vec::new()
	}

	fn handle_call(&self, _tool_name: &str, _arguments: Value) -> DynamicToolCallResponse {
		DynamicToolCallResponse::failure(String::from("unused"))
	}

	fn classify_turn_completion(&self, final_output: &str) -> Result<TurnCompletionStatus> {
		Ok(match final_output.trim() {
			"CONTINUE" => TurnCompletionStatus::Continue,
			"TERMINAL" => {
				self.terminal_seen.replace(true);

				TurnCompletionStatus::Complete
			},
			_ => TurnCompletionStatus::Complete,
		})
	}

	fn has_terminal_completion_signal(&self) -> bool {
		*self.terminal_seen.borrow()
	}
}

struct TestPhaseGoalController {
	initial_phase: PhaseGoalKind,
}
impl TestPhaseGoalController {
	fn new(initial_phase: PhaseGoalKind) -> Self {
		Self { initial_phase }
	}
}

impl PhaseGoalController for TestPhaseGoalController {
	fn initial_phase_goal(&self) -> Result<Option<PhaseGoalSpec>> {
		Ok(Some(PhaseGoalSpec::new(self.initial_phase, "test phase goal", None)))
	}

	fn phase_goal_completed(&self, phase: PhaseGoalKind) -> Result<PhaseGoalTransition> {
		Ok(match phase {
			PhaseGoalKind::ImplementToValidationReady
			| PhaseGoalKind::RepairValidationFailures
			| PhaseGoalKind::RepairAcceptedReviewFindings =>
				PhaseGoalTransition::Continue(PhaseGoalSpec::new(
					PhaseGoalKind::HandoffEvidence,
					"prepare handoff evidence",
					None,
				)),
			PhaseGoalKind::HandoffEvidence => PhaseGoalTransition::CompleteRun,
		})
	}
}

fn notification_message(method: &str, params: Value) -> WireMessage {
	WireMessage {
		raw: params.to_string(),
		message: JsonRpcMessage::Notification(JsonRpcNotification {
			method: method.to_owned(),
			params,
		}),
	}
}

#[test]
fn matches_thread_id_from_supported_notification_shapes() {
	for message in [
		notification_message(
			"thread/started",
			serde_json::json!({
				"thread": {
					"id": "thread-1",
				}
			}),
		),
		notification_message(
			"turn/completed",
			serde_json::json!({
				"threadId": "thread-1",
				"turn": {
					"id": "turn-1",
					"status": "completed",
					"error": null,
				}
			}),
		),
	] {
		assert!(super::targets_thread(&message, Some("thread-1")));
		assert!(!super::targets_thread(&message, Some("thread-2")));
	}
}

#[test]
fn probe_result_shape_is_stable() {
	let result = AppServerRunResult {
		user_agent: String::from("ua"),
		capability_preflight: AppServerCapabilityPreflightReport::new(),
		thread_id: String::from("thread"),
		turn_id: String::from("turn"),
		turn_count: 1,
		event_count: 3,
		final_output: String::from("PROBE_OK"),
		continuation_pending: false,
		phase_goal_status: None,
	};

	assert_eq!(result.final_output, "PROBE_OK");
	assert_eq!(result.turn_count, 1);
}

#[test]
fn generated_schema_marker_validation_accepts_required_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"description": "Decodex app-server compatibility fixture.",
			"requiredMarkers": super::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
			"properties": {
				"dynamicTools": {
					"properties": {
						"namespace": { "type": "string" },
						"deferLoading": { "type": "boolean" }
					}
				},
				"marketplaceKinds": { "type": "array" },
				"type": { "const": "inputText" }
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");
	super::validate_generated_app_server_schema(temp_dir.path())
		.expect("required markers should pass schema validation");
}

#[test]
fn generated_schema_marker_validation_rejects_missing_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"methods": ["initialize"]
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	let error = super::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("missing markers should fail schema validation");

	assert!(error.to_string().contains("missing required Decodex markers"));
	assert!(error.to_string().contains("turn/start"));
	assert!(error.to_string().contains("marketplaceKinds"));
}

#[test]
fn generated_schema_marker_validation_rejects_prose_only_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");
	let prose_markers = APP_SERVER_SCHEMA_REQUIRED_MARKERS.join(", ");

	fs::write(
		&schema_path,
		serde_json::json!({
			"description": prose_markers.clone(),
			"$comment": "Compatibility prose, not protocol structure.",
			"properties": {
				"documentationOnly": {
					"description": prose_markers
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	let error = super::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("prose-only markers should fail schema validation");

	assert!(error.to_string().contains("missing required Decodex markers"));
	assert!(error.to_string().contains("initialize"));
	assert!(error.to_string().contains("marketplaceKinds"));
}

#[test]
fn turn_start_request_uses_default_runtime_settings() {
	let request = super::build_turn_start_request("thread-1", "hello");

	assert_eq!(request.thread_id, "thread-1");
	assert!(matches!(
		request.input.as_slice(),
		[UserInput::Text{ text }] if text == "hello"
	));
}

#[test]
fn turn_steer_request_uses_expected_turn_precondition_and_text() {
	let request = super::build_turn_steer_request("thread-1", "turn-1", "change direction");

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

	let start =
		super::build_thread_start_request(&minimal_run_request()).expect("request should build");
	let start_value = serde_json::to_value(&start).expect("thread start request should serialize");
	let resume = super::build_thread_resume_request("thread-1", &minimal_run_request());
	let resume_value =
		serde_json::to_value(&resume).expect("thread resume request should serialize");

	assert_runtime_config(&start_value);
	assert_runtime_config(&resume_value);

	assert_eq!(resume_value["threadId"], "thread-1");
}

#[test]
fn thread_start_rejects_invalid_dynamic_tool_names() {
	let handler = InvalidToolNameHandler;
	let mut request = minimal_run_request();

	request.dynamic_tool_handler = Some(&handler);

	let error = super::build_thread_start_request(&request)
		.expect_err("invalid dynamic tool name should fail before thread/start");
	let failure = error
		.downcast_ref::<AppServerDynamicToolFailure>()
		.expect("invalid dynamic tool should classify as a dynamic tool failure");

	assert_eq!(failure.error_class(), "app_server_dynamic_tool_protocol_failure");
	assert!(error.to_string().contains("identifier pattern"));
}

#[test]
fn turn_start_request_omits_execution_policy_overrides() {
	let request = super::build_turn_start_request("thread-1", "hello");
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

fn minimal_run_request<'a>() -> super::AppServerRunRequest<'a> {
	super::AppServerRunRequest {
		project_id: String::from("test-project"),
		run_id: String::from("run-1"),
		issue_id: String::from("issue-1"),
		attempt_number: 1,
		listen: String::from("stdio://"),
		cwd: String::from("/tmp/worktree"),
		developer_instructions: String::from("Follow the workflow."),
		user_input: String::from("Work the issue."),
		max_turns: 1,
		timeout: Duration::from_secs(30),
		process_env: AppServerProcessEnv::default(),
		continuation_user_input: None,
		activity_marker_path: None,
		resume_thread_id: None,
		ephemeral_thread: false,
		command_exec_health_check: None,
		dynamic_tool_handler: None,
		continuation_guard: None,
		phase_goal_controller: None,
		codex_account_provider: None,
	}
}

#[test]
fn synthetic_probe_thread_start_is_ephemeral_when_requested() {
	let mut request = minimal_run_request();

	request.ephemeral_thread = true;

	let start = super::build_thread_start_request(&request).expect("request should build");
	let value = serde_json::to_value(&start).expect("thread start request should serialize");

	assert_eq!(value["ephemeral"], true);
}

#[test]
fn command_exec_health_check_uses_bounded_standalone_request() {
	let health_check = CommandExecHealthCheck {
		command: vec![String::from("/bin/sh"), String::from("-c"), String::from("printf ok")],
		expected_stdout: String::from("ok"),
		timeout_ms: 1_000,
		output_bytes_cap: 128,
	};
	let params = super::build_command_exec_health_check_params(&health_check, "/tmp/worktree");
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
	let fake_bin_dir = install_fake_codex_script(&temp_dir, orphan_response_fake_codex_script());
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("orphan-response-run");
	request.issue_id = String::from("orphan-response-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);
	request.activity_marker_path = Some(marker_path.clone());

	let result = super::execute_app_server_run(&request, &state_store)
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

fn install_fake_codex_script(temp_dir: &TempDir, script: &str) -> PathBuf {
	let fake_bin_dir = temp_dir.path().join("fake-bin");
	let fake_codex_path = fake_bin_dir.join("codex");

	fs::create_dir_all(&fake_bin_dir).expect("fake bin directory should create");
	fs::write(&fake_codex_path, script).expect("fake codex script should write");

	let mut permissions =
		fs::metadata(&fake_codex_path).expect("fake codex metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_codex_path, permissions)
		.expect("fake codex script should be executable");

	fake_bin_dir
}

fn orphan_response_fake_codex_script() -> &'static str {
	r#"#!/usr/bin/env python3
import json
import os
import sys

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params") or {}

    def reply(result):
        send({"id": message_id, "result": result})

    if method == "initialize":
        reply({
            "userAgent": "codex-cli 0.136.0",
            "codexHome": os.environ["CODEX_HOME"],
            "platformFamily": "unix",
            "platformOs": "macos"
        })
    elif method == "initialized":
        continue
    elif method == "config/read":
        reply({"config": {
            "model": "gpt-5.5",
            "model_provider": "openai",
            "approval_policy": {"type": "never"},
            "sandbox_mode": {"type": "dangerFullAccess"}
        }})
    elif method == "model/list":
        reply({"data": [{
            "id": "gpt-5.5",
            "model": "gpt-5.5",
            "displayName": "GPT-5.5",
            "isDefault": True,
            "hidden": False
        }], "nextCursor": None})
    elif method == "modelProvider/capabilities/read":
        reply({"imageGeneration": True, "namespaceTools": True, "webSearch": True})
    elif method == "skills/list":
        cwd = params.get("cwds", [""])[0]
        reply({"data": [{"cwd": cwd, "errors": [], "skills": [{
            "enabled": True,
            "name": "fake-skill",
            "scope": "user"
        }]}]})
    elif method == "plugin/list":
        reply({"marketplaces": [{"name": "fake", "plugins": [{
            "enabled": True,
            "id": "fake-plugin",
            "installed": True,
            "name": "Fake Plugin"
        }]}], "marketplaceLoadErrors": []})
    elif method == "mcpServerStatus/list":
        reply({"data": [], "nextCursor": None})
    elif method == "thread/start":
        cwd = params.get("cwd")
        reply({
            "thread": {"id": "thread-1"},
            "model": "gpt-5.5",
            "modelProvider": "openai",
            "serviceTier": None,
            "cwd": cwd,
            "instructionSources": [],
            "approvalPolicy": {"type": "never"},
            "approvalsReviewer": "user",
            "sandbox": {"type": "dangerFullAccess"},
            "reasoningEffort": None
        })
    elif method == "turn/start":
        reply({"turn": {"id": "turn-1", "status": "running", "error": None}})
        send({"method": "thread/status/changed", "params": {
            "threadId": "thread-1",
            "status": {"type": "active", "activeFlags": []}
        }})
        send({"method": "turn/started", "params": {
            "threadId": "thread-1",
            "turn": {"id": "turn-1", "status": "running", "error": None}
        }})
        send({"id": 999, "result": {"late": True}})
        send({"method": "item/completed", "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "item": {"type": "agentMessage", "text": "ORPHAN_OK"}
        }})
        send({"method": "turn/completed", "params": {
            "threadId": "thread-1",
            "turn": {"id": "turn-1", "status": "completed", "error": None}
        }})
    else:
        send({"id": message_id, "error": {
            "code": -32601,
            "message": "unexpected method " + str(method)
        }})
"#
}

fn phase_goal_fake_codex_script(
	turn_outputs: &[&str],
	goal_statuses: &[&str],
	unsupported_goal_methods: &[&str],
) -> String {
	let outputs_json = serde_json::to_string(turn_outputs).expect("turn outputs should serialize");
	let statuses_json =
		serde_json::to_string(goal_statuses).expect("goal statuses should serialize");
	let unsupported_goal =
		serde_json::to_string(unsupported_goal_methods).expect("methods should serialize");

	PHASE_GOAL_FAKE_CODEX_SCRIPT_TEMPLATE
		.replace("__TURN_OUTPUTS__", &outputs_json)
		.replace("__GOAL_STATUSES__", &statuses_json)
		.replace("__UNSUPPORTED_GOAL_METHODS__", &unsupported_goal)
}

fn execute_phase_goal_fake_app_server<'a, F>(
	script: String,
	configure: F,
) -> (Result<AppServerRunResult>, StateStore)
where
	F: FnOnce(&mut super::AppServerRunRequest<'a>),
{
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let fake_bin_dir = install_fake_codex_script(&temp_dir, &script);
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("phase-goal-run");
	request.issue_id = String::from("phase-goal-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);

	configure(&mut request);

	let result = super::execute_app_server_run(&request, &state_store);

	(result, state_store)
}

fn private_phase_goal_events(state_store: &StateStore, event_type: &str) -> Vec<Value> {
	state_store
		.list_private_execution_events("test-project", "phase-goal-issue", "phase-goal-run", 1)
		.expect("private phase goal events should load")
		.into_iter()
		.filter(|event| event.event_type() == event_type)
		.map(|event| event.payload().clone())
		.collect()
}

#[test]
fn phase_goal_set_method_is_required_when_phase_controller_is_present() {
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["DONE"], &[], &["thread/goal/set"]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("missing goal set support should fail immediately");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing goal support should be a typed phase-goal failure");

	assert_eq!(failure.error_class(), "app_server_phase_goal_unsupported");
	assert!(error.to_string().contains("thread/goal/set"));
}

#[test]
fn phase_goal_get_method_is_required_after_turn_completion() {
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["DONE"], &[], &["thread/goal/get"]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("missing goal get support should fail after the turn");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing goal support should be a typed phase-goal failure");

	assert_eq!(failure.error_class(), "app_server_phase_goal_unsupported");
	assert!(error.to_string().contains("thread/goal/get"));
}

#[test]
fn phase_goal_complete_runs_validation_transition_before_handoff_goal() {
	let handler = TerminalTokenCompletionHandler::default();
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script =
		phase_goal_fake_codex_script(&["DONE", "TERMINAL"], &["complete", "complete"], &[]);
	let (result, state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("completed phase goal should advance to handoff evidence goal");
	let completed_events = private_phase_goal_events(&state_store, "phase_goal_completed");
	let goal_set_events = private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.final_output, "TERMINAL");
	assert_eq!(
		result.phase_goal_status,
		Some(super::PhaseGoalRunStatus {
			phase: PhaseGoalKind::HandoffEvidence,
			status: String::from("complete"),
		})
	);
	assert_eq!(
		completed_events.iter().filter_map(|event| event["phase"].as_str()).collect::<Vec<_>>(),
		vec!["implement_to_validation_ready", "handoff_evidence"]
	);
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "handoff_evidence");
}

#[test]
fn still_active_phase_goal_stops_at_max_turns_without_terminal_signal() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["CONTINUE", "DONE"], &["active", "active"], &[]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 2;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("active goal should allow another bounded turn");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.turn_id, "turn-2");
	assert_eq!(result.final_output, "DONE");
	assert!(result.continuation_pending);
	assert_eq!(
		result.phase_goal_status,
		Some(super::PhaseGoalRunStatus {
			phase: PhaseGoalKind::ImplementToValidationReady,
			status: String::from("active"),
		})
	);
}

#[test]
fn still_active_phase_goal_stops_at_max_turns_with_continuation_pending() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["CONTINUE"], &["active"], &[]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 1;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("active goal should exit cleanly at max_turns");

	assert_eq!(result.turn_count, 1);
	assert!(result.continuation_pending);
	assert_eq!(
		result.phase_goal_status,
		Some(super::PhaseGoalRunStatus {
			phase: PhaseGoalKind::ImplementToValidationReady,
			status: String::from("active"),
		})
	);
}

#[test]
fn phase_goal_handoff_continue_without_terminal_completion_is_invalid() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::HandoffEvidence);
	let script = phase_goal_fake_codex_script(&["CONTINUE"], &["complete"], &[]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 2;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("handoff goal completion cannot replace terminal path");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing terminal path should be a typed phase-goal failure");

	assert_eq!(failure.error_class(), "phase_goal_terminal_path_missing");
	assert!(error.to_string().contains("handoff_evidence"));
}

#[test]
fn phase_goal_handoff_final_output_without_terminal_signal_is_invalid() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["DONE", "DONE"], &["complete", "complete"], &[]);
	let (result, state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let error = result.expect_err("handoff goal final output cannot replace terminal path");
	let failure = error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.expect("missing terminal path should be a typed phase-goal failure");
	let goal_set_events = private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(failure.error_class(), "phase_goal_terminal_path_missing");
	assert!(error.to_string().contains("handoff_evidence"));
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "handoff_evidence");
}

#[test]
fn archive_thread_after_success_calls_app_server_archive_and_records_event() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let fake_bin_dir = temp_dir.path().join("fake-bin");
	let fake_codex_path = fake_bin_dir.join("codex");
	let invocation_log_path = temp_dir.path().join("codex-invocations.jsonl");
	let invocation_log_literal =
		serde_json::to_string(&invocation_log_path).expect("log path should serialize");
	let fake_codex_script = format!(
		r#"#!/usr/bin/env python3
import json
import os
import sys

log_path = {invocation_log_literal}

with open(log_path, "a", encoding="utf-8") as log:
    log.write(json.dumps({{"args": sys.argv[1:]}}) + "\n")

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        print(json.dumps({{
            "id": message["id"],
            "result": {{
                "userAgent": "codex-cli 0.136.0",
                "codexHome": os.environ["CODEX_HOME"],
                "platformFamily": "unix",
                "platformOs": "macos"
            }}
        }}), flush=True)
    elif method == "initialized":
        continue
    elif method == "thread/archive":
        with open(log_path, "a", encoding="utf-8") as log:
            log.write(json.dumps(message, sort_keys=True) + "\n")
        print(json.dumps({{"id": message["id"], "result": {{}}}}), flush=True)
        print(json.dumps({{
            "method": "thread/archived",
            "params": {{"threadId": message["params"]["threadId"]}}
        }}), flush=True)
    else:
        print(json.dumps({{
            "id": message.get("id"),
            "error": {{"code": -32601, "message": "unexpected method " + str(method)}}
        }}), flush=True)
"#
	);

	fs::create_dir_all(&fake_bin_dir).expect("fake bin directory should create");
	fs::write(&fake_codex_path, fake_codex_script).expect("fake codex script should write");

	let mut permissions =
		fs::metadata(&fake_codex_path).expect("fake codex metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_codex_path, permissions)
		.expect("fake codex script should be executable");

	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("run attempt should record");

	let outcome = super::archive_app_server_thread_after_success(
		&AppServerThreadArchiveRequest {
			run_id: "run-1",
			issue_id: "issue-1",
			attempt_number: 1,
			listen: "stdio://",
			process_env: &AppServerProcessEnv::default(),
			thread_id: "thread-1",
			sequence_number: 1,
		},
		&state_store,
	)
	.expect("thread archive should succeed");
	let invocation_log =
		fs::read_to_string(&invocation_log_path).expect("invocation log should exist");

	assert!(invocation_log.contains(r#""app-server""#));
	assert!(invocation_log.contains(r#""--listen""#));
	assert!(invocation_log.contains(r#""method": "thread/archive""#));
	assert!(invocation_log.contains(r#""threadId": "thread-1""#));
	assert_eq!(outcome, AppServerThreadArchiveOutcome::Archived);
	assert!(
		state_store
			.run_has_protocol_event("run-1", "thread/archive")
			.expect("archive event lookup should succeed")
	);
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn missing_thread_archive_errors_record_discarded_terminal_event() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let process_env = AppServerProcessEnv::default();
	let request = AppServerThreadArchiveRequest {
		run_id: "run-1",
		issue_id: "issue-1",
		attempt_number: 1,
		listen: "stdio://",
		process_env: &process_env,
		thread_id: "thread-1",
		sequence_number: 1,
	};

	state_store
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("run attempt should record");

	super::record_thread_archive_result_best_effort(
		&state_store,
		&request,
		Ok(&AppServerThreadArchiveOutcome::DiscardedMissingThread),
	);

	assert!(super::thread_archive_error_allows_discard(&eyre::eyre!(
		"no rollout found for thread id thread-1"
	)));
	assert!(super::thread_archive_error_allows_discard(&eyre::eyre!("thread not found")));
	assert!(super::thread_archive_error_allows_discard(&eyre::eyre!("already archived")));
	assert!(!super::thread_archive_error_allows_discard(&eyre::eyre!(
		"failed to load rollout from disk"
	)));
	assert!(
		state_store
			.run_has_protocol_event("run-1", "thread/archive/discarded")
			.expect("discarded archive event lookup should succeed")
	);
	assert!(
		!state_store
			.run_has_protocol_event("run-1", "thread/archive/failed")
			.expect("failed archive event lookup should succeed")
	);
}

#[test]
fn command_exec_health_check_validates_exact_buffered_result() {
	let health_check = CommandExecHealthCheck {
		command: vec![String::from("/bin/sh"), String::from("-c"), String::from("printf ok")],
		expected_stdout: String::from("ok"),
		timeout_ms: 1_000,
		output_bytes_cap: 128,
	};
	let response =
		CommandExecResponse { exit_code: 0, stdout: String::from("ok"), stderr: String::new() };

	super::validate_command_exec_health_check_result(&health_check, &response)
		.expect("matching command exec result should pass");

	let bad_response =
		CommandExecResponse { exit_code: 0, stdout: String::from("wrong"), stderr: String::new() };
	let error = super::validate_command_exec_health_check_result(&health_check, &bad_response)
		.expect_err("mismatched stdout should fail health check");

	assert!(error.to_string().contains("expected \"ok\""));
}

#[test]
fn capability_preflight_report_accepts_available_runtime_state() {
	let config = RuntimeConfigSummary {
		model: Some(String::from("gpt-5.4")),
		model_provider: Some(String::from("openai")),
		approval_policy: Some(serde_json::json!("never")),
		sandbox_mode: Some(serde_json::json!("workspaceWrite")),
	};
	let models = vec![super::ModelSummary {
		id: String::from("model-gpt-5.4"),
		model: String::from("gpt-5.4"),
		display_name: String::from("GPT-5.4"),
		is_default: true,
		hidden: false,
	}];
	let capabilities = ModelProviderCapabilitiesReadResponse {
		image_generation: true,
		namespace_tools: true,
		web_search: true,
	};
	let skills = SkillsListResponse {
		data: vec![super::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: Vec::new(),
			skills: vec![super::protocol::SkillMetadata {
				enabled: true,
				name: String::from("playbook:rust"),
				scope: String::from("user"),
			}],
		}],
	};
	let plugins = PluginListResponse {
		marketplaces: vec![super::protocol::PluginMarketplaceEntry {
			name: String::from("curated"),
			plugins: vec![super::protocol::PluginSummary {
				enabled: true,
				id: String::from("github"),
				installed: true,
				name: String::from("GitHub"),
			}],
		}],
		marketplace_load_errors: Vec::new(),
	};
	let mcp = vec![super::McpServerStatusSummary {
		auth_status: String::from("bearerToken"),
		name: String::from("linear"),
		tools: BTreeMap::from([(String::from("issue_transition"), serde_json::json!({}))]),
	}];
	let mut report = AppServerCapabilityPreflightReport::new();

	super::record_config_preflight(&mut report, &config);
	super::record_model_preflight(&mut report, &config, &models);
	super::record_model_provider_preflight(&mut report, &capabilities);
	super::record_skills_preflight(&mut report, "/tmp/worktree", &skills);
	super::record_plugin_preflight(&mut report, &plugins);
	super::record_mcp_preflight(&mut report, &mcp);

	assert!(!report.has_blockers());
	assert_eq!(report.checks().len(), 6);
	assert!(
		report
			.checks()
			.iter()
			.all(|check| { check.status == super::AppServerCapabilityPreflightStatus::Ok })
	);

	let serialized = serde_json::to_value(&report).expect("report should serialize");

	assert_eq!(serialized["checks"][0]["status"], "ok");
	assert_eq!(serialized["checks"][1]["details"]["configured_model"], "gpt-5.4");
}

#[test]
fn capability_preflight_report_allows_enabled_skills_with_scan_diagnostics() {
	let skills = SkillsListResponse {
		data: vec![super::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: vec![super::protocol::SkillErrorInfo {
				message: String::from("name: exceeds maximum length of 64 characters"),
				path: String::from(
					"/tmp/plugins/build-web-data-visualization/skills/chart/SKILL.md",
				),
			}],
			skills: vec![super::protocol::SkillMetadata {
				enabled: true,
				name: String::from("playbook:rust"),
				scope: String::from("user"),
			}],
		}],
	};
	let mut report = AppServerCapabilityPreflightReport::new();

	super::record_skills_preflight(&mut report, "/tmp/worktree", &skills);

	assert!(!report.has_blockers());
	assert_eq!(report.checks()[0].status, super::AppServerCapabilityPreflightStatus::Ok);
	assert_eq!(
		report.checks()[0].summary,
		"skills/list returned enabled skills with scan diagnostics."
	);
	assert_eq!(report.checks()[0].details["enabled_skill_count"], "1");
	assert_eq!(report.checks()[0].details["error_count"], "1");
	assert_eq!(
		report.checks()[0].details["first_error"],
		"name: exceeds maximum length of 64 characters"
	);
}

#[test]
fn capability_preflight_report_blocks_missing_runtime_state() {
	let config = RuntimeConfigSummary {
		model: Some(String::from("missing-model")),
		model_provider: Some(String::from("openai")),
		approval_policy: None,
		sandbox_mode: None,
	};
	let models = vec![super::ModelSummary {
		id: String::from("model-gpt-5.4"),
		model: String::from("gpt-5.4"),
		display_name: String::from("GPT-5.4"),
		is_default: true,
		hidden: false,
	}];
	let skills = SkillsListResponse {
		data: vec![super::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: vec![super::protocol::SkillErrorInfo {
				message: String::from("bad skill metadata"),
				path: String::from("/tmp/worktree/.codex/skills/bad/SKILL.md"),
			}],
			skills: Vec::new(),
		}],
	};
	let plugins = PluginListResponse {
		marketplaces: Vec::new(),
		marketplace_load_errors: vec![super::protocol::MarketplaceLoadErrorInfo {
			marketplace_path: String::from("/tmp/plugins.json"),
			message: String::from("invalid marketplace"),
		}],
	};
	let mcp = vec![super::McpServerStatusSummary {
		auth_status: String::from("notLoggedIn"),
		name: String::from("linear"),
		tools: BTreeMap::new(),
	}];
	let mut report = AppServerCapabilityPreflightReport::new();

	super::record_model_preflight(&mut report, &config, &models);
	super::record_skills_preflight(&mut report, "/tmp/worktree", &skills);
	super::record_plugin_preflight(&mut report, &plugins);
	super::record_mcp_preflight(&mut report, &mcp);

	assert!(report.has_blockers());
	assert_eq!(
		report.blocker_summary(),
		"model: configured model was not present in model/list.; skills: skills/list returned no enabled skills. first_error_path=/tmp/worktree/.codex/skills/bad/SKILL.md; first_error=bad skill metadata; plugins: plugin/list returned marketplace load errors. first_error_path=/tmp/plugins.json; first_error=invalid marketplace; mcp: mcpServerStatus/list returned MCP servers that are not logged in."
	);
}

#[test]
fn plugin_list_preflight_uses_local_marketplaces() {
	let params = super::plugin_list_params_for_preflight("/tmp/worktree");
	let serialized = serde_json::to_value(&params).expect("plugin params should serialize");

	assert_eq!(serialized["cwds"], serde_json::json!(["/tmp/worktree"]));
	assert_eq!(serialized["marketplaceKinds"], serde_json::json!(["local"]));
}

#[test]
fn capability_preflight_method_error_is_typed_operator_blocker() {
	let mut report = AppServerCapabilityPreflightReport::new();

	report.push_ok(
		"config",
		"config/read returned effective runtime configuration.",
		BTreeMap::new(),
	);

	let failure = AppServerCapabilityPreflightFailure::method_failed(
		"model/list",
		String::from("`model/list` failed with -32601: Method not found"),
		report,
	);

	assert_eq!(failure.error_class(), "app_server_introspection_method_failed");
	assert!(!failure.is_retryable_timeout());
	assert!(failure.to_string().contains("model/list"));
	assert!(failure.to_string().contains("Method not found"));
	assert_eq!(failure.report().checks().len(), 1);
}

#[test]
fn capability_preflight_request_error_records_method_blocker() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let error = super::preflight_request::<(), _>(&mut recorder, &report, "model/list", || {
		Err(eyre::eyre!("JSON-RPC error -32601: Method not found"))
	})
	.expect_err("unsupported app-server method should fail preflight");
	let failure = error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.expect("preflight request error should be typed");

	assert_eq!(failure.error_class(), "app_server_introspection_method_failed");
	assert!(failure.to_string().contains("model/list"));
	assert!(failure.to_string().contains("Method not found"));
	assert!(failure.report().has_blockers());
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn plugin_list_preflight_timeout_retries_once_before_success() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let mut attempts = 0;
	let response = super::preflight_request_with_timeout_retry(
		&mut recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		2,
		|| {
			attempts += 1;

			if attempts == 1 { Err(Report::new(AppServerOutputTimeout)) } else { Ok("plugins-ok") }
		},
	)
	.expect("second plugin/list attempt should recover");

	assert_eq!(response, "plugins-ok");
	assert_eq!(attempts, 2);
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 0);
}

#[test]
fn plugin_list_preflight_timeout_failure_is_typed_retryable_timeout() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let mut attempts = 0;
	let error = super::preflight_request_with_timeout_retry::<(), _>(
		&mut recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		2,
		|| {
			attempts += 1;

			Err(Report::new(AppServerOutputTimeout))
		},
	)
	.expect_err("exhausted plugin/list timeout should fail preflight");
	let failure = error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.expect("plugin/list timeout should be typed");
	let check = &failure.report().checks()[0];
	let timeout_seconds = REQUEST_TIMEOUT.as_secs().to_string();

	assert_eq!(attempts, 2);
	assert_eq!(failure.error_class(), "app_server_plugin_list_timeout");
	assert!(failure.is_retryable_timeout());
	assert!(failure.to_string().contains("app_server_preflight_failed"));
	assert!(failure.to_string().contains("plugin/list"));
	assert!(failure.to_string().contains("timed out"));
	assert!(failure.retry_next_action().contains("retry app-server preflight automatically"));
	assert!(failure.report().has_blockers());
	assert_eq!(check.name, "plugins");
	assert_eq!(check.status, super::AppServerCapabilityPreflightStatus::Blocked);
	assert_eq!(check.details.get("failure_reason").map(String::as_str), Some("timeout"));
	assert_eq!(check.details.get("attempt_count").map(String::as_str), Some("2"));
	assert_eq!(check.details.get("retry_count").map(String::as_str), Some("1"));
	assert_eq!(
		check.details.get("timeout_seconds").map(String::as_str),
		Some(timeout_seconds.as_str())
	);
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn mcp_preflight_timeout_degrades_to_recorded_ok_check() {
	let error = Report::new(AppServerOutputTimeout);
	let mut report = AppServerCapabilityPreflightReport::new();

	assert!(super::mcp_preflight_can_degrade(&error));

	super::record_mcp_preflight_degraded(&mut report, &error);

	assert!(!report.has_blockers());
	assert_eq!(report.checks().len(), 1);
	assert_eq!(report.checks()[0].name, "mcp");
	assert_eq!(report.checks()[0].status, super::AppServerCapabilityPreflightStatus::Ok);
	assert_eq!(
		report.checks()[0].details.get("degraded_reason").map(String::as_str),
		Some("timeout")
	);
	assert!(report.checks()[0].summary.contains("continuing"));
}

#[test]
fn remaining_idle_budget_resets_from_latest_activity() {
	let now = Instant::now();
	let timeout = Duration::from_secs(300);
	let last_activity_at = now.checked_sub(Duration::from_secs(12)).expect("instant math");
	let remaining =
		super::remaining_idle_budget(last_activity_at, now, timeout).expect("budget should remain");

	assert!(remaining <= timeout);
	assert!(remaining >= Duration::from_secs(287));
}

#[test]
fn remaining_idle_budget_expires_after_idle_timeout() {
	let now = Instant::now();
	let timeout = Duration::from_secs(300);
	let last_activity_at = now.checked_sub(Duration::from_secs(301)).expect("instant math");

	assert!(super::remaining_idle_budget(last_activity_at, now, timeout).is_none());
}

#[test]
fn protocol_activity_idle_timeout_extends_running_model_execution() {
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		..ProtocolActivitySummary::default()
	};

	assert_eq!(
		super::protocol_activity_idle_timeout(
			Some(&protocol_activity),
			super::ACTIVE_RUN_IDLE_TIMEOUT
		),
		super::MODEL_EXECUTION_IDLE_TIMEOUT
	);
}

#[test]
fn protocol_activity_idle_timeout_keeps_base_timeout_for_other_waits() {
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("tool_execution")),
		..ProtocolActivitySummary::default()
	};

	assert_eq!(
		super::protocol_activity_idle_timeout(
			Some(&protocol_activity),
			super::ACTIVE_RUN_IDLE_TIMEOUT
		),
		super::ACTIVE_RUN_IDLE_TIMEOUT
	);
}

#[test]
fn run_recorder_keeps_events_when_marker_write_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let missing_worktree = PathBuf::from(temp_dir.path()).join("missing-worktree");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&missing_worktree));

	recorder.mark_activity().expect("marker failures should be non-fatal");
	recorder.record("turn/started", "{\"turn\":\"1\"}").expect("event should record");

	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn completion_classification_uses_dynamic_tool_handler() {
	let error = super::classify_turn_completion(Some(&RejectingCompletionHandler), "finished")
		.expect_err("completion classifier should be consulted");

	assert!(error.to_string().contains("terminal finalization missing"));
}

#[test]
fn completion_classification_defaults_to_complete_without_handler() {
	assert_eq!(
		super::classify_turn_completion(None, "finished")
			.expect("missing dynamic handler should not fail completion"),
		TurnCompletionStatus::Complete
	);
}

#[test]
fn probe_handler_allows_completion_classification() {
	assert_eq!(
		super::classify_turn_completion(Some(&ProbeDynamicToolHandler), "PROBE_OK")
			.expect("probe handler should not override completion validation"),
		TurnCompletionStatus::Complete
	);
}

#[test]
fn nonterminal_single_turn_completion_stays_invalid() {
	let error = super::reject_nonterminal_single_turn_completion(
		Some(&ContinuingCompletionHandler),
		"unfinished",
	)
	.expect_err("single-turn mode should preserve terminal completion validation");

	assert!(error.to_string().contains("terminal finalization missing"));
}

#[test]
fn continuation_boundary_reached_yields_when_guard_allows_it() {
	assert!(
		super::continuation_boundary_reached(Some(&YieldingContinuationGuard), 2)
			.expect("yielding guard should allow a clean continuation boundary")
	);
}

#[test]
fn continuation_boundary_reached_rejects_invalid_boundary() {
	let error = super::continuation_boundary_reached(Some(&RejectingContinuationGuard), 1)
		.expect_err("invalid continuation boundaries should surface as errors");

	assert!(error.to_string().contains("turn 1 hit an invalid continuation boundary"));
}

#[test]
fn validate_effective_thread_config_accepts_noninteractive_runtime() {
	let runtime = EffectiveThreadConfig {
		model: String::from("gpt-5.4"),
		model_provider: String::from("openai"),
		cwd: String::from("/tmp/worktree"),
		approval_policy: String::from("never"),
		approvals_reviewer: String::from("human"),
		sandbox_mode: String::from("workspaceWrite"),
	};

	super::validate_effective_thread_config("/tmp/worktree", &runtime)
		.expect("matching non-interactive config should validate");
}

#[test]
fn validate_effective_thread_config_rejects_interactive_runtime_policies() {
	for (case_name, approval_policy, sandbox_mode, expected) in [
		(
			"interactive approval policy",
			"onRequest",
			"workspaceWrite",
			"approval policy `onRequest`",
		),
		("read-only sandbox", "never", "readOnly", "readOnly"),
	] {
		let runtime = EffectiveThreadConfig {
			model: String::from("gpt-5.4"),
			model_provider: String::from("openai"),
			cwd: String::from("/tmp/worktree"),
			approval_policy: String::from(approval_policy),
			approvals_reviewer: String::from("human"),
			sandbox_mode: String::from(sandbox_mode),
		};
		let error = super::validate_effective_thread_config("/tmp/worktree", &runtime)
			.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn initialize_codex_home_assertion_accepts_expected_home() {
	let expected = ResolvedAppServerCodexHomeEnv::new(
		PathBuf::from("/Users/test/.codex"),
		PathBuf::from("/Users/test/.codex"),
	)
	.expect("test Codex home should validate");
	let response = InitializeResponse {
		user_agent: String::from("codex-cli-test"),
		codex_home: String::from("/Users/test/.codex"),
		platform_family: String::from("unix"),
		platform_os: String::from("macos"),
	};

	super::validate_initialize_codex_home(&expected, &response)
		.expect("matching Codex home should pass");
}

#[test]
fn initialize_codex_home_assertion_blocks_before_thread_start_on_mismatch() {
	let expected = ResolvedAppServerCodexHomeEnv::new(
		PathBuf::from("/Users/test/.codex"),
		PathBuf::from("/Users/test/.codex"),
	)
	.expect("test Codex home should validate");
	let response = InitializeResponse {
		user_agent: String::from("codex-cli-test"),
		codex_home: String::from("/tmp/per-account-codex-home"),
		platform_family: String::from("unix"),
		platform_os: String::from("macos"),
	};
	let error = super::validate_initialize_codex_home(&expected, &response)
		.expect_err("mismatched Codex home should fail before thread start");

	assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
	assert!(error.to_string().contains("initialize codexHome `/tmp/per-account-codex-home`"));
	assert!(error.to_string().contains("expected shared Codex home `/Users/test/.codex`"));
	assert!(error.to_string().contains("before thread/start"));
}

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

#[test]
fn steer_response_wait_ignores_temp_file_until_atomic_response_exists() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let request = LaneControlSteerRequest::new(LaneControlSteerRequestInput {
		audit_record_id: 7,
		project_id: "decodex",
		issue_id: "XY-704",
		run_id: "run-1",
		attempt_number: 1,
		thread_id: "thread-1",
		expected_turn_id: "turn-1",
		source: "test",
		message: "change direction",
	});
	let run_dir = temp_dir.path().join(".decodex-run-control").join("run-1");

	fs::create_dir_all(&run_dir)?;
	fs::write(run_dir.join(format!("{}.steer-response.json.tmp", request.request_id)), b"{")?;

	assert!(
		run_control::wait_for_steer_response(
			temp_dir.path(),
			"run-1",
			&request.request_id,
			Duration::from_millis(1),
		)?
		.is_none()
	);

	let response = LaneControlSteerResponse::delivered(&request, "turn-1", "turn-2");

	run_control::write_steer_response(temp_dir.path(), &response)?;

	assert_eq!(
		run_control::wait_for_steer_response(
			temp_dir.path(),
			"run-1",
			&request.request_id,
			Duration::from_millis(100),
		)?,
		Some(response)
	);

	Ok(())
}

#[test]
fn thread_resume_fallback_only_allows_missing_thread_errors() {
	assert!(super::thread_resume_error_allows_fallback(&eyre::eyre!("thread not found")));
	assert!(super::thread_resume_error_allows_fallback(&eyre::eyre!(
		"no rollout found for thread id thread-1"
	)));
	assert!(!super::thread_resume_error_allows_fallback(&eyre::eyre!(
		"failed to load rollout from disk"
	)));
	assert!(!super::thread_resume_error_allows_fallback(&eyre::eyre!(
		"thread belongs to another cwd"
	)));
}

#[test]
fn dynamic_tool_call_enforces_declared_namespace() {
	for (case_name, namespace, expected_success, expected_seen_namespace, expected_error) in [
		(
			"unknown namespace",
			Some("other"),
			false,
			None,
			Some(
				"Dynamic tool `tracker_tool` was called under namespace `other`, but this run did not declare that tool namespace.",
			),
		),
		("declared namespace", Some("tracker"), true, Some("tracker"), None),
		(
			"missing namespace",
			None,
			false,
			None,
			Some("Dynamic tool `tracker_tool` is not declared for this run attempt."),
		),
	] {
		let handler = NamespacedDynamicToolHandler { seen_namespace: RefCell::new(None) };
		let mut params = serde_json::json!({
			"arguments": {},
			"callId": "call-1",
			"threadId": "thread-1",
			"tool": "tracker_tool",
			"turnId": "turn-1"
		});

		if let Some(namespace) = namespace {
			params["namespace"] = serde_json::json!(namespace);
		}

		let request = JsonRpcRequest {
			id: serde_json::json!(1),
			method: String::from("item/tool/call"),
			params,
		};
		let dispatch =
			super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", Some("turn-1"));

		assert_eq!(dispatch.response.success, expected_success, "{case_name}");
		assert_eq!(
			*handler.seen_namespace.borrow(),
			expected_seen_namespace.map(String::from),
			"{case_name}"
		);

		if let Some(expected_error) = expected_error {
			assert_eq!(
				dispatch.response.content_items,
				vec![DynamicToolContentItem::InputText { text: String::from(expected_error) }],
				"{case_name}"
			);
			assert_eq!(
				dispatch
					.terminal_failure
					.as_ref()
					.map(super::AppServerDynamicToolFailure::error_class),
				Some("app_server_dynamic_tool_protocol_failure"),
				"{case_name}"
			);
		} else {
			assert!(dispatch.terminal_failure.is_none(), "{case_name}");
		}
	}
}

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
	let dispatch =
		super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", Some("active-turn"));

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
	let dispatch =
		super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", Some("active-turn"));

	assert!(!dispatch.response.success);
	assert_eq!(*handler.seen_namespace.borrow(), None);
	assert_eq!(
		dispatch.terminal_failure.as_ref().map(super::AppServerDynamicToolFailure::error_class),
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
	let dispatch =
		super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", Some("turn-1"));

	assert!(!dispatch.response.success);
	assert!(matches!(
		dispatch.response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("invalid response with no `contentItems`")
	));
	assert_eq!(
		dispatch.terminal_failure.as_ref().map(super::AppServerDynamicToolFailure::error_class),
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
	let dispatch =
		super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", Some("turn-1"));

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
	let dispatch = super::handle_dynamic_tool_call(Some(&handler), &request, "thread-1", None);

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
	let error = super::handle_turn_execution_notification(
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

#[test]
fn turn_notification_ignores_agent_output_for_non_target_turn() {
	let old_completed = JsonRpcNotification {
		method: String::from("item/completed"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-old",
			"item": {"type": "agentMessage", "text": "OLD"}
		}),
	};
	let old_delta = JsonRpcNotification {
		method: String::from("item/agentMessage/delta"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-old",
			"delta": " OLD_DELTA"
		}),
	};
	let target_completed = JsonRpcNotification {
		method: String::from("item/completed"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-new",
			"item": {"type": "agentMessage", "text": "NEW"}
		}),
	};
	let mut final_output = String::from("CURRENT");
	let mut latest_turn_failure: Option<AppServerTurnFailure> = None;

	assert!(
		super::handle_turn_execution_notification(
			&old_completed,
			"thread-1",
			"turn-new",
			&mut final_output,
			&mut latest_turn_failure
		)
		.expect("old completed notification should parse")
		.is_none()
	);
	assert!(
		super::handle_turn_execution_notification(
			&old_delta,
			"thread-1",
			"turn-new",
			&mut final_output,
			&mut latest_turn_failure
		)
		.expect("old delta notification should parse")
		.is_none()
	);
	assert_eq!(final_output, "CURRENT");

	super::handle_turn_execution_notification(
		&target_completed,
		"thread-1",
		"turn-new",
		&mut final_output,
		&mut latest_turn_failure,
	)
	.expect("target completed notification should parse");

	assert_eq!(final_output, "NEW");
}

#[test]
fn dynamic_tool_call_unavailable_outside_turn_execution_is_protocol_diagnostic() {
	let dispatch = super::dynamic_tool_call_unavailable_for_phase(RequestWaitPhase::TurnStart);

	assert!(!dispatch.response.success);
	assert!(matches!(
		dispatch.response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("unavailable while waiting for turn/start")
	));
	assert_eq!(
		dispatch.terminal_failure.as_ref().map(super::AppServerDynamicToolFailure::error_class),
		Some("app_server_dynamic_tool_protocol_failure")
	);

	let diagnostic = dispatch.diagnostic.expect("protocol failure should publish a diagnostic");

	assert_eq!(diagnostic.failure_class, "app_server_dynamic_tool_protocol_failure");
	assert!(diagnostic.message.contains("unavailable while waiting for turn/start"));
	assert_eq!(
		diagnostic.next_action,
		"inspect the declared dynamic tool surface and item/tool/call payload before retrying the lane"
	);
}

#[test]
fn interactive_request_updates_marker_turn_id_to_current_turn() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder.set_thread_id("thread-1").expect("thread marker should write");
	recorder.set_turn_id("turn-old").expect("initial turn marker should write");

	let request = JsonRpcRequest {
		id: serde_json::json!(1),
		method: String::from("item/tool/requestUserInput"),
		params: serde_json::json!({
			"threadId": "thread-1",
			"turnId": "turn-new",
		}),
	};

	super::record_interactive_request_state(&mut recorder, &request)
		.expect("interactive request state should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.thread_id(), Some("thread-1"));
	assert_eq!(marker.turn_id(), Some("turn-new"));
	assert_eq!(marker.thread_status(), Some("active"));
	assert_eq!(marker.thread_active_flags(), &[String::from("waitingOnUserInput")]);
}

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

#[test]
fn recorder_summarizes_high_value_protocol_activity() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	for (event_type, payload) in [
		(
			"turn/started",
			r#"{"method":"turn/started","params":{"turn":{"id":"turn-1","status":"running"}}}"#,
		),
		("plan/update", r#"{"method":"plan/update","params":{"step":"verify"}}"#),
		("diff/update", r#"{"method":"diff/update","params":{"filesChanged":2}}"#),
		(
			"item/tool/call/failure",
			r#"{"failureClass":"app_server_dynamic_tool_failed","tool":"issue_comment","message":"tool rejected","nextAction":"retry"}"#,
		),
		("command/output/delta", r#"{"method":"command/output/delta","params":{"delta":"ok"}}"#),
		("item/tool/requestUserInput/response", r#"{"answers":{}}"#),
		(
			"item/tool/requestUserInput",
			r#"{"method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1"}}"#,
		),
		(
			"account/rateLimit/update",
			r#"{"rateLimitReachedType":"primary","primaryRemainingPercent":0}"#,
		),
	] {
		recorder.record(event_type, payload).expect("protocol event should record");
	}

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let categories =
		summary.recent_events.iter().map(|event| event.category.as_str()).collect::<Vec<_>>();

	assert_eq!(summary.turn_status.as_deref(), Some("running"));
	assert_eq!(summary.waiting_reason.as_deref(), Some("approval_or_user_input"));
	assert_eq!(summary.rate_limit_status.as_deref(), Some("primary"));
	assert!(categories.contains(&"turn"));
	assert!(categories.contains(&"plan"));
	assert!(categories.contains(&"diff"));
	assert!(categories.contains(&"item"));
	assert!(categories.contains(&"command_output"));
	assert!(categories.contains(&"protocol_error"));
	assert!(categories.contains(&"server_request_resolution"));
	assert!(categories.contains(&"rate_limit"));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "item/tool/call/failure"
			&& event.detail.as_deref() == Some("app_server_dynamic_tool_failed")
	}));
}

#[test]
fn recorder_summarizes_v2_account_rate_limit_notifications() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/rateLimits/updated",
			r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"planType":"pro","rateLimitReachedType":"workspace_member_usage_limit_reached","primary":{"usedPercent":100}}}}"#,
		)
		.expect("rate limit protocol event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let event = summary.recent_events.first().expect("recent rate limit event should render");

	assert_eq!(summary.rate_limit_status.as_deref(), Some("workspace_member_usage_limit_reached"));
	assert_eq!(event.category, "rate_limit");
	assert_eq!(event.detail.as_deref(), Some("pro/workspace_member_usage_limit_reached"));
}

#[test]
fn recorder_summarizes_codex_app_server_warning_and_model_notifications() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	for (event_type, payload) in [
		(
			"deprecationNotice",
			r#"{"method":"deprecationNotice","params":{"summary":"persistExtendedHistory is ignored","details":"Remove the request field."}}"#,
		),
		(
			"configWarning",
			r#"{"method":"configWarning","params":{"summary":"unknown feature key in config","details":"builtin_mcp"}}"#,
		),
		(
			"model/rerouted",
			r#"{"method":"model/rerouted","params":{"threadId":"thread-1","turnId":"turn-1","fromModel":"gpt-5.4","toModel":"gpt-5.5","reason":"highRiskCyberActivity"}}"#,
		),
		(
			"model/verification",
			r#"{"method":"model/verification","params":{"threadId":"thread-1","turnId":"turn-1","verifications":["trustedAccessForCyber"]}}"#,
		),
		(
			"thread/tokenUsage/updated",
			r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"thread-1","turnId":"turn-1","tokenUsage":{"last":{"inputTokens":10,"cachedInputTokens":0,"outputTokens":5,"reasoningOutputTokens":1,"totalTokens":16},"total":{"inputTokens":100,"cachedInputTokens":12,"outputTokens":30,"reasoningOutputTokens":8,"totalTokens":138},"modelContextWindow":200000}}}"#,
		),
	] {
		recorder.record(event_type, payload).expect("protocol event should record");
	}

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let categories =
		summary.recent_events.iter().map(|event| event.category.as_str()).collect::<Vec<_>>();

	assert!(categories.contains(&"deprecation"));
	assert!(categories.contains(&"warning"));
	assert!(categories.contains(&"model"));
	assert!(categories.contains(&"token_usage"));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "deprecationNotice"
			&& event.detail.as_deref() == Some("persistExtendedHistory is ignored")
	}));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "model/rerouted"
			&& event.detail.as_deref() == Some("gpt-5.4->gpt-5.5/highRiskCyberActivity")
	}));
	assert!(summary.recent_events.iter().any(|event| {
		event.event_type == "thread/tokenUsage/updated"
			&& event.detail.as_deref() == Some("input=100, output=30")
	}));
}

#[test]
fn recorder_does_not_treat_rate_limit_update_method_as_limit_status() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/rateLimits/updated",
			r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"planType":"pro","rateLimitReachedType":null,"primary":{"usedPercent":12}}}}"#,
		)
		.expect("rate limit update event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.rate_limit_status, None);
	assert_eq!(
		summary.recent_events.first().and_then(|event| event.detail.as_deref()),
		Some("pro")
	);
}

#[test]
fn recorder_summarizes_wrapped_account_protocol_activity() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/update",
			r#"{"method":"account/update","params":{"planType":"pro","refreshStatus":"refreshed"}}"#,
		)
		.expect("account protocol event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let event = summary.recent_events.first().expect("recent account event should render");

	assert_eq!(event.category, "account");
	assert_eq!(event.detail.as_deref(), Some("pro/refreshed"));
}

#[test]
fn turn_execution_records_dynamic_tool_call_before_response() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let request = JsonRpcRequest {
		id: serde_json::json!(7),
		method: String::from("item/tool/call"),
		params: serde_json::json!({
			"tool": "issue_progress_checkpoint",
			"arguments": {"phase": "verifying"},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"callId": "call-1",
		}),
	};
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	super::record_server_request(&mut recorder, &request)
		.expect("tool call request should record before handler execution");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let activity = marker.child_agent_activity().expect("child activity should be captured");

	assert_eq!(marker.last_event_type(), Some("item/tool/call"));
	assert_eq!(activity.current_bucket.as_deref(), Some("Tracker"));
	assert_eq!(activity.current_detail.as_deref(), Some("issue_progress_checkpoint"));
	assert!(activity.buckets.iter().any(|bucket| {
		bucket.name == "Tracker" && bucket.tool_call_count == 1 && bucket.event_count == 1
	}));
}

#[test]
#[ignore = "requires a live local codex app-server binary"]
fn live_app_server_resume_round_trip_updates_marker_and_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let marker_path = temp_dir.path().to_path_buf();
	let first_state_store = StateStore::open_in_memory().expect("state store should open");
	let handler = LiveResumeDynamicToolHandler;
	let guard = LiveResumeBoundaryGuard;
	let cwd = marker_path.display().to_string();
	let developer_instructions = String::from(
		"You are a live resume integration test. On the first turn, call the dynamic tool `echo_resume` exactly once with the JSON argument `{\"text\":\"FIRST_OK\"}` and then reply with the exact text CONTINUE. If the thread is later resumed and the user asks for `SECOND_OK`, call `echo_resume` exactly once with `{\"text\":\"SECOND_OK\"}` and then reply with the exact text DONE. Do not use shell. Do not inspect files.",
	);
	let first_result = super::execute_app_server_run(
		&super::AppServerRunRequest {
			project_id: String::from("test-project"),
			run_id: String::from("live-resume-run"),
			issue_id: String::from("live-resume-issue"),
			attempt_number: 1,
			listen: String::from("stdio://"),
			cwd: cwd.clone(),
			developer_instructions: developer_instructions.clone(),
			user_input: String::from(
				"Call `echo_resume` with `{\\\"text\\\":\\\"FIRST_OK\\\"}`. After the tool succeeds, reply with the exact text CONTINUE.",
			),
				max_turns: 3,
				timeout: Duration::from_secs(30),
				process_env: AppServerProcessEnv::default(),
				continuation_user_input: Some(String::from(
				"Call `echo_resume` with `{\\\"text\\\":\\\"SECOND_OK\\\"}`. After the tool succeeds, reply with the exact text DONE.",
			)),
			activity_marker_path: Some(marker_path.clone()),
			resume_thread_id: None,
			ephemeral_thread: false,
			command_exec_health_check: None,
				dynamic_tool_handler: Some(&handler),
				continuation_guard: Some(&guard),
				phase_goal_controller: None,
				codex_account_provider: None,
			},
		&first_state_store,
	)
	.expect("first live app-server run should succeed");

	assert!(first_result.continuation_pending);
	assert_eq!(first_result.final_output.trim(), "CONTINUE");

	let first_marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("first marker snapshot should load")
		.expect("first marker snapshot should exist");

	assert_eq!(first_marker.run_id(), "live-resume-run");
	assert_eq!(first_marker.attempt_number(), 1);
	assert_eq!(first_marker.thread_id(), Some(first_result.thread_id.as_str()));
	assert_eq!(first_marker.turn_id(), Some(first_result.turn_id.as_str()));
	assert_eq!(first_marker.effective_cwd(), Some(cwd.as_str()));
	assert_eq!(first_marker.effective_approval_policy(), Some("never"));
	assert!(first_marker.last_protocol_activity_unix_epoch().is_some());

	let resumed_state_store =
		StateStore::open_in_memory().expect("resumed state store should open");
	let second_result = super::execute_app_server_run(
		&super::AppServerRunRequest {
			project_id: String::from("test-project"),
			run_id: String::from("live-resume-run"),
			issue_id: String::from("live-resume-issue"),
			attempt_number: 1,
			listen: String::from("stdio://"),
			cwd: cwd.clone(),
			developer_instructions,
			user_input: String::from(
				"Call `echo_resume` with `{\\\"text\\\":\\\"SECOND_OK\\\"}`. After the tool succeeds, reply with the exact text DONE.",
			),
				max_turns: 1,
				timeout: Duration::from_secs(30),
				process_env: AppServerProcessEnv::default(),
				continuation_user_input: None,
			activity_marker_path: Some(marker_path.clone()),
			resume_thread_id: Some(first_result.thread_id.clone()),
			ephemeral_thread: false,
			command_exec_health_check: None,
				dynamic_tool_handler: Some(&handler),
				continuation_guard: None,
				phase_goal_controller: None,
				codex_account_provider: None,
			},
		&resumed_state_store,
	)
	.expect("resumed live app-server run should succeed");

	assert!(!second_result.continuation_pending);
	assert_eq!(second_result.thread_id, first_result.thread_id);
	assert_ne!(second_result.turn_id, first_result.turn_id);
	assert_eq!(second_result.final_output.trim(), "DONE");

	let second_marker = state::read_run_activity_marker_snapshot(&marker_path)
		.expect("second marker snapshot should load")
		.expect("second marker snapshot should exist");

	assert_eq!(second_marker.thread_id(), Some(first_result.thread_id.as_str()));
	assert_eq!(second_marker.turn_id(), Some(second_result.turn_id.as_str()));
	assert_eq!(second_marker.effective_model_provider(), Some("openai"));
	assert_eq!(second_marker.effective_cwd(), Some(cwd.as_str()));
	assert!(second_marker.last_protocol_activity_unix_epoch().is_some());
	assert!(second_marker.event_count() > 0);

	let resumed_attempt = resumed_state_store
		.run_attempt("live-resume-run")
		.expect("resumed run attempt should load")
		.expect("resumed run attempt should exist");

	assert_eq!(resumed_attempt.thread_id(), Some(first_result.thread_id.as_str()));
	assert_eq!(resumed_attempt.turn_id(), Some(second_result.turn_id.as_str()));
}
