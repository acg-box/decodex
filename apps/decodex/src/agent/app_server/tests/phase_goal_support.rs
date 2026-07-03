use std::{cell::RefCell, env, fs, time::Duration};

use serde_json::Value;
use tempfile::TempDir;

use crate::{
	agent::{
		app_server::{
			AppServerRunRequest, AppServerRunResult, PhaseGoalController, PhaseGoalKind,
			PhaseGoalSpec, PhaseGoalTransition, tests,
		},
		tracker_tool_bridge::{
			DynamicToolCallResponse, DynamicToolHandler, DynamicToolSpec, TurnCompletionStatus,
		},
	},
	prelude::Result,
	state::StateStore,
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

pub(super) struct ContinueTokenCompletionHandler;
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
pub(super) struct TerminalTokenCompletionHandler {
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

pub(super) struct TestPhaseGoalController {
	initial_phase: PhaseGoalKind,
}
impl TestPhaseGoalController {
	pub(super) fn new(initial_phase: PhaseGoalKind) -> Self {
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

pub(super) fn phase_goal_fake_codex_script(
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

pub(super) fn phase_goal_fake_codex_script_with_notification_turn_mismatch(
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

pub(super) fn execute_phase_goal_fake_app_server<'a, F>(
	script: String,
	configure: F,
) -> (Result<AppServerRunResult>, StateStore)
where
	F: FnOnce(&mut AppServerRunRequest<'a>),
{
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("worktree");
	let fake_bin_dir = tests::install_fake_codex_script(&temp_dir, &script);
	let path_env = env::var("PATH").unwrap_or_default();
	let _path_guard =
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_bin_dir.display()));
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut request = tests::minimal_run_request();

	fs::create_dir_all(&worktree_path).expect("worktree directory should create");

	request.run_id = String::from("phase-goal-run");
	request.issue_id = String::from("phase-goal-issue");
	request.cwd = worktree_path.display().to_string();
	request.timeout = Duration::from_secs(5);

	configure(&mut request);

	let result = super::execute_app_server_run(&request, &state_store);

	(result, state_store)
}

pub(super) fn private_phase_goal_events(state_store: &StateStore, event_type: &str) -> Vec<Value> {
	state_store
		.list_private_execution_events("test-project", "phase-goal-issue", "phase-goal-run", 1)
		.expect("private phase goal events should load")
		.into_iter()
		.filter(|event| event.event_type() == event_type)
		.map(|event| event.payload().clone())
		.collect()
}
