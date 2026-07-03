mod archive;
mod dynamic_tools;
mod phase_goal_runtime;
mod phase_goal_support;
mod phase_goal_tests;
mod preflight;
mod recorder;
mod request_tests;
mod runtime;
mod schema_tests;

use std::{
	cell::RefCell,
	fs,
	os::unix::fs::PermissionsExt,
	path::{Path, PathBuf},
	time::Duration,
};

use serde_json::{self, Value};
use tempfile::TempDir;

use crate::{
	agent::{
		app_server::{
			APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
			APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
			APP_SERVER_SCHEMA_REQUIRED_MARKERS, AppServerCapabilityPreflightFailure,
			AppServerCapabilityPreflightReport, AppServerThreadArchiveOutcome,
			AppServerThreadArchiveRequest, AppServerTurnFailure, CommandExecHealthCheck,
			CommandExecResponse, EffectiveThreadConfig, InitializeResponse,
			ModelProviderCapabilitiesReadResponse, PluginListResponse, ProbeDynamicToolHandler,
			REQUEST_TIMEOUT, RunRecorder, RuntimeConfigSummary, SkillsListResponse,
			TurnContinuationGuard, archive_app_server_thread_after_success,
			classify_turn_completion, continuation_boundary_reached, execute_app_server_run,
			failure_from_error_notification, handle_dynamic_tool_call,
			handle_turn_execution_notification, mcp_preflight_can_degrade,
			plugin_list_params_for_preflight, preflight_request,
			preflight_request_with_timeout_retry, protocol_activity_idle_timeout,
			record_config_preflight, record_interactive_request_state, record_mcp_preflight,
			record_mcp_preflight_degraded, record_model_preflight, record_model_provider_preflight,
			record_plugin_preflight, record_server_request, record_skills_preflight,
			record_thread_archive_result_best_effort, reject_nonterminal_single_turn_completion,
			remaining_idle_budget, steer_error_class, thread_archive_error_allows_discard,
			thread_resume_error_allows_fallback, turn_failure_from_json_rpc_error_response,
			validate_command_exec_health_check_result, validate_effective_thread_config,
			validate_initialize_codex_home,
		},
		json_rpc::{
			AppServerHomePreflightFailure, AppServerProcessEnv, JsonRpcError, JsonRpcErrorPayload,
			JsonRpcMessage, JsonRpcNotification, ResolvedAppServerCodexHomeEnv, WireMessage,
		},
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::{Result, eyre},
	run_control::{
		LaneControlSteerRequest, LaneControlSteerRequestInput, LaneControlSteerResponse,
	},
};
use phase_goal_support::{
	ContinueTokenCompletionHandler, TerminalTokenCompletionHandler, TestPhaseGoalController,
	execute_phase_goal_fake_app_server, phase_goal_fake_codex_script,
	phase_goal_fake_codex_script_with_notification_turn_mismatch, private_phase_goal_events,
};

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

fn notification_message(method: &str, params: Value) -> WireMessage {
	WireMessage {
		raw: params.to_string(),
		message: JsonRpcMessage::Notification(JsonRpcNotification {
			method: method.to_owned(),
			params,
		}),
	}
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

fn slow_thread_start_fake_codex_script() -> String {
	orphan_response_fake_codex_script().replace(
		"    elif method == \"thread/start\":\n        cwd = params.get(\"cwd\")",
		"    elif method == \"thread/start\":\n        import time\n        time.sleep(6)\n        cwd = params.get(\"cwd\")",
	)
}

fn retrying_error_fake_codex_script() -> String {
	orphan_response_fake_codex_script().replace(
		"        send({\"id\": 999, \"result\": {\"late\": True}})",
		r#"        send({"method": "error", "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "willRetry": True,
            "error": {
                "message": "Reconnecting... 2/5",
                "codexErrorInfo": "transientNetworkError"
            }
        }})
        send({"id": 999, "result": {"late": True}})"#,
	)
}
