use std::env;

use crate::{
	agent::{
		app_server::{
			self,
			constants::{
				PROBE_DEVELOPER_INSTRUCTIONS, PROBE_EXPECTED_OUTPUT, PROBE_ISSUE_ID, PROBE_RUN_ID,
				PROBE_TIMEOUT, PROBE_USER_INPUT,
			},
			preflight::CommandExecHealthCheck,
			protocol::ProbeDynamicToolHandler,
			runtime_types::{AppServerRunRequest, AppServerRunResult},
			schema_probe,
		},
		json_rpc::AppServerProcessEnv,
	},
	prelude::{Result, eyre},
	state::StateStore,
};

pub(crate) fn probe_app_server(listen: &str) -> Result<AppServerRunResult> {
	let state_store = StateStore::open_in_memory()?;
	let probe_tool_handler = ProbeDynamicToolHandler;

	schema_probe::probe_app_server_schema(&AppServerProcessEnv::default())?;

	let result = app_server::execute_app_server_run(
		&AppServerRunRequest {
			project_id: String::from("probe"),
			run_id: PROBE_RUN_ID.to_owned(),
			issue_id: PROBE_ISSUE_ID.to_owned(),
			attempt_number: 1,
			listen: listen.to_owned(),
			cwd: env::current_dir()?.display().to_string(),
			developer_instructions: PROBE_DEVELOPER_INSTRUCTIONS.to_owned(),
			user_input: PROBE_USER_INPUT.to_owned(),
			max_turns: 1,
			timeout: PROBE_TIMEOUT,
			process_env: AppServerProcessEnv::default(),
			continuation_user_input: None,
			activity_marker_path: None,
			resume_thread_id: None,
			ephemeral_thread: true,
			command_exec_health_check: Some(CommandExecHealthCheck::probe()),
			dynamic_tool_handler: Some(&probe_tool_handler),
			continuation_guard: None,
			phase_goal_controller: None,
			codex_account_provider: None,
		},
		&state_store,
	)?;

	if result.final_output.trim() != PROBE_EXPECTED_OUTPUT {
		eyre::bail!(
			"Protocol probe completed, but the final output was `{}` instead of `{PROBE_EXPECTED_OUTPUT}`.",
			result.final_output.trim()
		);
	}

	Ok(result)
}
