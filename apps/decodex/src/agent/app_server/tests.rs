use std::{
	cell::RefCell,
	collections::BTreeMap,
	env, fs,
	os::unix::fs::PermissionsExt,
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde_json::{self, Value};
use tempfile::TempDir;

use crate::{
	agent::{
		app_server::{
			APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
			APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
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
MISMATCH_NOTIFICATION_TURN_IDS = __MISMATCH_NOTIFICATION_TURN_IDS__

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
        notification_turn_id = "notification-" + turn_id if MISMATCH_NOTIFICATION_TURN_IDS else turn_id
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
            "turn": {"id": notification_turn_id, "status": "running", "error": None},
        }})
        if not unsupported_goal_method("thread/goal/updated"):
            send({"method": "thread/goal/updated", "params": {
                "threadId": "thread-1",
                "turnId": notification_turn_id,
                "goal": goal_payload("active"),
            }})
        send({"method": "item/completed", "params": {
            "threadId": "thread-1",
            "turnId": notification_turn_id,
            "item": {"type": "agentMessage", "text": output},
        }})
        send({"method": "turn/completed", "params": {
            "threadId": "thread-1",
            "turn": {"id": notification_turn_id, "status": "completed", "error": None},
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
			PhaseGoalKind::RepairAcceptedReviewFindings =>
				PhaseGoalTransition::Continue(PhaseGoalSpec::new(
					PhaseGoalKind::ReviewRepairEvidence,
					"prepare review repair evidence",
					None,
				)),
			PhaseGoalKind::ImplementToValidationReady | PhaseGoalKind::RepairValidationFailures =>
				PhaseGoalTransition::Continue(PhaseGoalSpec::new(
					PhaseGoalKind::HandoffEvidence,
					"prepare handoff evidence",
					None,
				)),
			PhaseGoalKind::ReviewRepairEvidence | PhaseGoalKind::HandoffEvidence =>
				PhaseGoalTransition::CompleteRun,
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
			"title": "ThreadStartParams",
			"properties": {
				"dynamicTools": {
					"items": {
						"$ref": "#/definitions/DynamicToolSpec"
					}
				},
				"marketplaceKinds": { "type": "array" },
				"type": { "const": "inputText" }
			},
			"definitions": {
				"DynamicToolNamespaceTool": {
					"oneOf": [{
						"title": "FunctionDynamicToolNamespaceTool",
						"required": ["description", "inputSchema", "name", "type"],
						"properties": {
							"deferLoading": { "type": "boolean" },
							"description": { "type": "string" },
							"inputSchema": true,
							"name": { "type": "string" },
							"type": { "enum": ["function"] }
						}
					}]
				},
				"DynamicToolSpec": {
					"oneOf": [
						{
							"title": "FunctionDynamicToolSpec",
							"required": ["description", "inputSchema", "name", "type"],
							"properties": {
								"deferLoading": { "type": "boolean" },
								"description": { "type": "string" },
								"inputSchema": true,
								"name": { "type": "string" },
								"type": { "enum": ["function"] }
							}
						},
						{
							"title": "NamespaceDynamicToolSpec",
							"required": ["description", "name", "tools", "type"],
							"properties": {
								"description": { "type": "string" },
								"name": { "type": "string" },
								"tools": {
									"items": {
										"$ref": "#/definitions/DynamicToolNamespaceTool"
									}
								},
								"type": { "enum": ["namespace"] }
							}
						}
					]
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	write_app_server_method_union_fixtures(temp_dir.path(), None);

	super::validate_generated_app_server_schema(temp_dir.path())
		.expect("required markers should pass schema validation");
}

fn write_app_server_method_union_fixtures(root: &Path, omitted: Option<(&str, &str)>) {
	for (title, required_methods) in [
		("ClientRequest", APP_SERVER_REQUIRED_CLIENT_REQUESTS),
		("ServerRequest", APP_SERVER_REQUIRED_SERVER_REQUESTS),
		("ClientNotification", APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS),
		("ServerNotification", APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS),
	] {
		let branches = required_methods
			.iter()
			.filter(|(method, _schema)| omitted != Some((title, *method)))
			.map(|(method, schema)| {
				let mut properties = serde_json::json!({
					"method": {
						"type": "string",
						"enum": [method]
					}
				});

				if !schema.is_empty() {
					properties["params"] = serde_json::json!({
						"$ref": format!("#/definitions/{schema}")
					});
				}

				serde_json::json!({
					"title": format!("{method}Fixture"),
					"type": "object",
					"properties": properties
				})
			})
			.collect::<Vec<_>>();

		fs::write(
			root.join(format!("{title}.json")),
			serde_json::json!({
				"title": title,
				"oneOf": branches
			})
			.to_string(),
		)
		.expect("schema union fixture should write");
	}
}

#[test]
fn generated_schema_marker_validation_rejects_legacy_flat_dynamic_tools() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"requiredMarkers": super::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
			"title": "ThreadStartParams",
			"properties": {
				"dynamicTools": {
					"items": {
						"$ref": "#/definitions/DynamicToolSpec"
					}
				}
			},
			"definitions": {
				"DynamicToolSpec": {
					"type": "object",
					"required": ["description", "inputSchema", "name"],
					"properties": {
						"deferLoading": { "type": "boolean" },
						"description": { "type": "string" },
						"inputSchema": true,
						"name": { "type": "string" },
						"namespace": { "type": ["string", "null"] }
					}
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	let error = super::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("legacy flat dynamicTools should fail schema validation");

	assert!(error.to_string().contains("0.141 dynamicTools tagged union"));
}

#[test]
fn generated_schema_marker_validation_rejects_missing_owned_method() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"requiredMarkers": super::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
			"title": "ThreadStartParams",
			"properties": {
				"dynamicTools": {
					"items": {
						"$ref": "#/definitions/DynamicToolSpec"
					}
				}
			},
			"definitions": {
				"DynamicToolNamespaceTool": {
					"oneOf": [{
						"title": "FunctionDynamicToolNamespaceTool",
						"required": ["description", "inputSchema", "name", "type"],
						"properties": {
							"description": { "type": "string" },
							"inputSchema": true,
							"name": { "type": "string" },
							"type": { "enum": ["function"] }
						}
					}]
				},
				"DynamicToolSpec": {
					"oneOf": [
						{
							"title": "FunctionDynamicToolSpec",
							"required": ["description", "inputSchema", "name", "type"],
							"properties": {
								"description": { "type": "string" },
								"inputSchema": true,
								"name": { "type": "string" },
								"type": { "enum": ["function"] }
							}
						},
						{
							"title": "NamespaceDynamicToolSpec",
							"required": ["description", "name", "tools", "type"],
							"properties": {
								"description": { "type": "string" },
								"name": { "type": "string" },
								"tools": {
									"items": {
										"$ref": "#/definitions/DynamicToolNamespaceTool"
									}
								},
								"type": { "enum": ["namespace"] }
							}
						}
					]
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	write_app_server_method_union_fixtures(temp_dir.path(), Some(("ClientRequest", "turn/start")));

	let error = super::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("missing Decodex-owned method should fail schema validation");

	assert!(error.to_string().contains("ClientRequest"));
	assert!(error.to_string().contains("turn/start missing"));
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
	let mut request = minimal_run_request();

	request.dynamic_tool_handler = Some(&handler);

	let start = super::build_thread_start_request(&request).expect("request should build");
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
	phase_goal_fake_codex_script_with_notification_turn_mismatch(
		turn_outputs,
		goal_statuses,
		unsupported_goal_methods,
		false,
	)
}

fn phase_goal_fake_codex_script_with_notification_turn_mismatch(
	turn_outputs: &[&str],
	goal_statuses: &[&str],
	unsupported_goal_methods: &[&str],
	mismatch_notification_turn_ids: bool,
) -> String {
	let outputs_json = serde_json::to_string(turn_outputs).expect("turn outputs should serialize");
	let statuses_json =
		serde_json::to_string(goal_statuses).expect("goal statuses should serialize");
	let unsupported_goal =
		serde_json::to_string(unsupported_goal_methods).expect("methods should serialize");
	let mismatch_turn_ids = if mismatch_notification_turn_ids { "True" } else { "False" };

	PHASE_GOAL_FAKE_CODEX_SCRIPT_TEMPLATE
		.replace("__TURN_OUTPUTS__", &outputs_json)
		.replace("__GOAL_STATUSES__", &statuses_json)
		.replace("__UNSUPPORTED_GOAL_METHODS__", &unsupported_goal)
		.replace("__MISMATCH_NOTIFICATION_TURN_IDS__", mismatch_turn_ids)
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
fn phase_goal_review_repair_validation_transitions_before_repair_evidence_goal() {
	let handler = TerminalTokenCompletionHandler::default();
	let controller = TestPhaseGoalController::new(PhaseGoalKind::RepairAcceptedReviewFindings);
	let script =
		phase_goal_fake_codex_script(&["DONE", "TERMINAL"], &["complete", "complete"], &[]);
	let (result, state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result =
		result.expect("completed review repair goal should advance to repair evidence goal");
	let completed_events = private_phase_goal_events(&state_store, "phase_goal_completed");
	let goal_set_events = private_phase_goal_events(&state_store, "phase_goal_set");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.final_output, "TERMINAL");
	assert_eq!(
		result.phase_goal_status,
		Some(super::PhaseGoalRunStatus {
			phase: PhaseGoalKind::ReviewRepairEvidence,
			status: String::from("complete"),
		})
	);
	assert_eq!(
		completed_events.iter().filter_map(|event| event["phase"].as_str()).collect::<Vec<_>>(),
		vec!["repair_accepted_review_findings", "review_repair_evidence"]
	);
	assert_eq!(goal_set_events.len(), 2);
	assert_eq!(goal_set_events[1]["phase"], "review_repair_evidence");
}

#[test]
fn phase_goal_completion_accepts_thread_bound_notification_turn_alias() {
	let handler = TerminalTokenCompletionHandler::default();
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script_with_notification_turn_mismatch(
		&["DONE", "TERMINAL"],
		&["complete", "complete"],
		&[],
		true,
	);
	let (result, state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 3;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("thread-bound turn alias should still complete phase goals");
	let completed_events = private_phase_goal_events(&state_store, "phase_goal_completed");
	let run_attempt = state_store
		.run_attempt("phase-goal-run")
		.expect("run attempt should load")
		.expect("run attempt should exist");

	assert_eq!(result.turn_count, 2);
	assert_eq!(result.turn_id, "notification-turn-2");
	assert_eq!(run_attempt.turn_id(), Some("notification-turn-2"));
	assert_eq!(
		completed_events.iter().filter_map(|event| event["phase"].as_str()).collect::<Vec<_>>(),
		vec!["implement_to_validation_ready", "handoff_evidence"]
	);
}

#[test]
fn open_phase_goal_stops_at_max_turns_without_terminal_signal() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["CONTINUE", "DONE"], &["active", "active"], &[]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 2;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("open phase goal should allow another bounded turn");

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
fn open_phase_goal_stops_at_max_turns_with_continuation_pending() {
	let handler = ContinueTokenCompletionHandler;
	let controller = TestPhaseGoalController::new(PhaseGoalKind::ImplementToValidationReady);
	let script = phase_goal_fake_codex_script(&["CONTINUE"], &["active"], &[]);
	let (result, _state_store) = execute_phase_goal_fake_app_server(script, |request| {
		request.max_turns = 1;
		request.dynamic_tool_handler = Some(&handler);
		request.phase_goal_controller = Some(&controller);
	});
	let result = result.expect("open phase goal should exit cleanly at max_turns");

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

mod archive;
mod dynamic_tools;
mod preflight;
mod recorder;
mod runtime;
