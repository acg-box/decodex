use std::{env, fs, time::Duration};

use tempfile::TempDir;

use crate::{
	agent::app_server::{self, tests},
	state::StateStore,
	test_support::TestEnvVarGuard,
};

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
