use std::{env, fs, os::unix::fs::PermissionsExt};

use tempfile::TempDir;

use crate::{
	agent::{
		app_server::tests::{
			AppServerThreadArchiveOutcome, AppServerThreadArchiveRequest, CommandExecHealthCheck,
			CommandExecResponse,
		},
		json_rpc::AppServerProcessEnv,
	},
	prelude::eyre,
	state::StateStore,
	test_support::TestEnvVarGuard,
};
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
