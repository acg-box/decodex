use std::{path::PathBuf, time::Duration};

use crate::agent::{
	app_server::{
		CodexAccountProvider, CommandExecHealthCheck, DynamicToolHandler, PhaseGoalController,
		TurnContinuationGuard,
	},
	json_rpc::AppServerProcessEnv,
};

pub(crate) struct AppServerThreadArchiveRequest<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) listen: &'a str,
	pub(crate) process_env: &'a AppServerProcessEnv,
	pub(crate) thread_id: &'a str,
	pub(crate) sequence_number: i64,
}

#[derive(Clone)]
pub(crate) struct AppServerRunRequest<'a> {
	pub(crate) project_id: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) listen: String,
	pub(crate) cwd: String,
	pub(crate) developer_instructions: String,
	pub(crate) user_input: String,
	pub(crate) max_turns: u32,
	pub(crate) timeout: Duration,
	pub(crate) process_env: AppServerProcessEnv,
	pub(crate) continuation_user_input: Option<String>,
	pub(crate) activity_marker_path: Option<PathBuf>,
	pub(crate) resume_thread_id: Option<String>,
	pub(crate) ephemeral_thread: bool,
	pub(crate) command_exec_health_check: Option<CommandExecHealthCheck>,
	pub(crate) dynamic_tool_handler: Option<&'a dyn DynamicToolHandler>,
	pub(crate) continuation_guard: Option<&'a dyn TurnContinuationGuard>,
	pub(crate) phase_goal_controller: Option<&'a dyn PhaseGoalController>,
	pub(crate) codex_account_provider: Option<&'a dyn CodexAccountProvider>,
}
