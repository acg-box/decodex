use std::{env, fs, time::Duration};

use serde_json::Value;
use tempfile::TempDir;

use crate::{
	agent::{
		app_server,
		app_server::{AppServerRunRequest, AppServerRunResult, tests},
	},
	prelude::Result,
	state::StateStore,
	test_support::TestEnvVarGuard,
};

pub(in crate::agent::app_server::tests) fn execute_phase_goal_fake_app_server<'a, F>(
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

	let result = app_server::execute_app_server_run(&request, &state_store);

	(result, state_store)
}

pub(in crate::agent::app_server::tests) fn private_phase_goal_events(
	state_store: &StateStore,
	event_type: &str,
) -> Vec<Value> {
	state_store
		.list_private_execution_events("test-project", "phase-goal-issue", "phase-goal-run", 1)
		.expect("private phase goal events should load")
		.into_iter()
		.filter(|event| event.event_type() == event_type)
		.map(|event| event.payload().clone())
		.collect()
}
