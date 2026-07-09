use std::{collections::HashMap, ffi::OsString, fs, path::PathBuf, process::Command};

use crate::{
	active_run_env::{
		ActiveRunCommitContext, DECODEX_ACTIVE_RUN_ID_ENV, DECODEX_ACTIVE_RUN_ISSUE_ID_ENV,
		DECODEX_ACTIVE_RUN_SERVICE_ID_ENV,
	},
	agent::json_rpc::{
		AppServerHomePreflightFailure, AppServerProcessEnv, ResolvedAppServerCodexHomeEnv,
		environment,
	},
};

#[test]
fn app_server_command_inherits_noninteractive_git_environment() {
	let process_env = AppServerProcessEnv::with_github_credentials(
		String::from("GITHUB_PAT_Y"),
		String::from("ghp_test_token"),
	);
	let mut command = Command::new("codex");

	environment::configure_app_server_command(&mut command, "stdio://", &process_env)
		.expect("app-server command should configure");

	let args = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
	let envs = command_envs(&command);

	assert_eq!(args, ["app-server", "--listen", "stdio://"]);
	assert_eq!(envs.get("GH_TOKEN").map(String::as_str), Some("ghp_test_token"));
	assert_eq!(envs.get("GITHUB_TOKEN").map(String::as_str), Some("ghp_test_token"));
	assert_eq!(envs.get("GITHUB_PAT_Y").map(String::as_str), Some("ghp_test_token"));
	assert_eq!(envs.get("GH_PROMPT_DISABLED").map(String::as_str), Some("1"));
	assert_eq!(envs.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
	assert_eq!(envs.get("GCM_INTERACTIVE").map(String::as_str), Some("never"));
	assert!(!envs.contains_key("GIT_ASKPASS"));
	assert_eq!(envs.get("GIT_CONFIG_COUNT").map(String::as_str), Some("11"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_0").map(String::as_str), Some("credential.helper"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_0").map(String::as_str), Some(""));
	assert_eq!(envs.get("GIT_CONFIG_KEY_1").map(String::as_str), Some("credential.helper"));
	assert!(
		envs.get("GIT_CONFIG_VALUE_1")
			.is_some_and(|value| value.contains("github.com") && value.contains("x-access-token"))
	);
	assert_eq!(
		envs.get("GIT_CONFIG_KEY_2").map(String::as_str),
		Some("url.https://github.com/.insteadOf")
	);
	assert_eq!(envs.get("GIT_CONFIG_VALUE_2").map(String::as_str), Some("git@github.com:"));
	assert_eq!(
		envs.get("GIT_CONFIG_KEY_7").map(String::as_str),
		Some("url.https://github.com/.insteadOf")
	);
	assert_eq!(envs.get("GIT_CONFIG_VALUE_7").map(String::as_str), Some("ssh://git@github.com-y/"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_8").map(String::as_str), Some("commit.gpgsign"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_8").map(String::as_str), Some("false"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_9").map(String::as_str), Some("tag.gpgsign"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_9").map(String::as_str), Some("false"));
	assert_eq!(envs.get("GIT_CONFIG_KEY_10").map(String::as_str), Some("user.signingkey"));
	assert_eq!(envs.get("GIT_CONFIG_VALUE_10").map(String::as_str), Some(""));
}

#[test]
fn app_server_command_inherits_active_run_commit_context() {
	let process_env =
		AppServerProcessEnv::default().with_active_run_commit_context(ActiveRunCommitContext::new(
			String::from("decodex"),
			String::from("xy-1247-attempt-1"),
			String::from("issue-1"),
		));
	let mut command = Command::new("codex");

	environment::configure_app_server_command(&mut command, "stdio://", &process_env)
		.expect("app-server command should configure");

	let envs = command_envs(&command);

	assert_eq!(envs.get(DECODEX_ACTIVE_RUN_SERVICE_ID_ENV).map(String::as_str), Some("decodex"));
	assert_eq!(envs.get(DECODEX_ACTIVE_RUN_ID_ENV).map(String::as_str), Some("xy-1247-attempt-1"));
	assert_eq!(envs.get(DECODEX_ACTIVE_RUN_ISSUE_ID_ENV).map(String::as_str), Some("issue-1"));
}

#[test]
fn app_server_program_falls_back_to_home_local_codex_when_path_is_sparse() {
	let temp_dir = tempfile::tempdir().expect("tempdir should create");
	let local_bin = temp_dir.path().join(".local/bin");

	fs::create_dir_all(&local_bin).expect("local bin should create");

	let codex_path = local_bin.join("codex");

	fs::write(&codex_path, "#!/bin/sh\n").expect("codex fixture should write");

	let resolved = environment::app_server_command_program_from_env(
		Some(OsString::from("/usr/bin:/bin")),
		Some(temp_dir.path().as_os_str().to_owned()),
	);

	assert_eq!(resolved, codex_path);
}

#[test]
fn app_server_command_does_not_rewrite_git_urls_without_credentials() {
	let mut command = Command::new("codex");

	environment::configure_app_server_command(
		&mut command,
		"stdio://",
		&AppServerProcessEnv::default(),
	)
	.expect("app-server command should configure");

	let envs = command_envs(&command);

	assert_eq!(envs.get("GH_PROMPT_DISABLED").map(String::as_str), Some("1"));
	assert_eq!(envs.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
	assert_eq!(envs.get("GCM_INTERACTIVE").map(String::as_str), Some("never"));
	assert!(!envs.contains_key("GIT_CONFIG_COUNT"));
	assert!(!envs.keys().any(|key| key.starts_with("GIT_CONFIG_KEY_")));
	assert!(!envs.keys().any(|key| key.starts_with("GIT_CONFIG_VALUE_")));
}

#[test]
fn app_server_command_sets_shared_codex_homes() {
	let shared_home = PathBuf::from("/Users/test/.codex");
	let home_env = ResolvedAppServerCodexHomeEnv::new(shared_home.clone(), shared_home.clone())
		.expect("test home should validate");
	let process_env = AppServerProcessEnv::with_codex_home_for_test(home_env);
	let mut command = Command::new("codex");
	let resolved =
		environment::configure_app_server_command(&mut command, "stdio://", &process_env)
			.expect("app-server command should configure");
	let envs = command_envs(&command);

	assert_eq!(resolved.codex_home(), shared_home.as_path());
	assert_eq!(envs.get("CODEX_HOME").map(String::as_str), Some("/Users/test/.codex"));
	assert_eq!(envs.get("CODEX_SQLITE_HOME").map(String::as_str), Some("/Users/test/.codex"));
}

#[test]
fn shared_codex_home_resolution_uses_home_dot_codex_for_state() {
	let resolved =
		environment::resolve_shared_codex_home_env_from_home(Some(OsString::from("/Users/test")))
			.expect("absolute HOME should resolve");

	assert_eq!(resolved.codex_home(), PathBuf::from("/Users/test/.codex").as_path());
	assert_eq!(resolved.sqlite_home(), PathBuf::from("/Users/test/.codex").as_path());
}

#[test]
fn app_server_command_overrides_ambient_codex_home_leakage() {
	let shared_home = PathBuf::from("/Users/test/.codex");
	let home_env = ResolvedAppServerCodexHomeEnv::new(shared_home.clone(), shared_home)
		.expect("test home should validate");
	let process_env = AppServerProcessEnv::with_codex_home_for_test(home_env);
	let mut command = Command::new("codex");

	command.env("CODEX_HOME", "/tmp/per-account-codex-home");
	command.env("CODEX_SQLITE_HOME", "/tmp/per-account-codex-state");

	environment::configure_app_server_command(&mut command, "stdio://", &process_env)
		.expect("app-server command should configure");

	let envs = command_envs(&command);

	assert_eq!(envs.get("CODEX_HOME").map(String::as_str), Some("/Users/test/.codex"));
	assert_eq!(envs.get("CODEX_SQLITE_HOME").map(String::as_str), Some("/Users/test/.codex"));
}

#[test]
fn shared_codex_home_resolution_requires_home() {
	let error = environment::resolve_shared_codex_home_env_from_home(None)
		.expect_err("missing HOME should fail");

	assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
	assert!(error.to_string().contains("HOME is not set"));
}

#[test]
fn shared_codex_home_resolution_rejects_invalid_home() {
	for (case_name, home, expected) in [
		("empty", OsString::from(""), "HOME is empty"),
		("relative", OsString::from("relative-home"), "is not absolute"),
	] {
		let error =
			environment::resolve_shared_codex_home_env_from_home(Some(home)).expect_err(case_name);

		assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
		assert!(
			error.to_string().contains(expected),
			"unexpected error for {case_name}: {error:?}"
		);
	}
}

fn command_envs(command: &Command) -> HashMap<String, String> {
	command
		.get_envs()
		.filter_map(|(key, value)| {
			Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
		})
		.collect()
}
