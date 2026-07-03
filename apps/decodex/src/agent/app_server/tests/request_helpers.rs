use std::time::Duration;

use serde_json::Value;

use crate::agent::{
	app_server::AppServerRunRequest,
	json_rpc::{AppServerProcessEnv, JsonRpcMessage, JsonRpcNotification, WireMessage},
};

pub(super) fn notification_message(method: &str, params: Value) -> WireMessage {
	WireMessage {
		raw: params.to_string(),
		message: JsonRpcMessage::Notification(JsonRpcNotification {
			method: method.to_owned(),
			params,
		}),
	}
}

pub(super) fn minimal_run_request<'a>() -> AppServerRunRequest<'a> {
	AppServerRunRequest {
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
