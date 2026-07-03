use std::path::PathBuf;

use crate::agent::app_server::tests::{
	AppServerHomePreflightFailure, EffectiveThreadConfig, InitializeResponse,
	ResolvedAppServerCodexHomeEnv,
};

#[test]
fn validate_effective_thread_config_accepts_noninteractive_runtime() {
	let runtime = EffectiveThreadConfig {
		model: String::from("gpt-5.4"),
		model_provider: String::from("openai"),
		cwd: String::from("/tmp/worktree"),
		approval_policy: String::from("never"),
		approvals_reviewer: String::from("human"),
		sandbox_mode: String::from("workspaceWrite"),
	};

	super::validate_effective_thread_config("/tmp/worktree", &runtime)
		.expect("matching non-interactive config should validate");
}

#[test]
fn validate_effective_thread_config_rejects_interactive_runtime_policies() {
	for (case_name, approval_policy, sandbox_mode, expected) in [
		(
			"interactive approval policy",
			"onRequest",
			"workspaceWrite",
			"approval policy `onRequest`",
		),
		("read-only sandbox", "never", "readOnly", "readOnly"),
	] {
		let runtime = EffectiveThreadConfig {
			model: String::from("gpt-5.4"),
			model_provider: String::from("openai"),
			cwd: String::from("/tmp/worktree"),
			approval_policy: String::from(approval_policy),
			approvals_reviewer: String::from("human"),
			sandbox_mode: String::from(sandbox_mode),
		};
		let error = super::validate_effective_thread_config("/tmp/worktree", &runtime)
			.expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn initialize_codex_home_assertion_accepts_expected_home() {
	let expected = ResolvedAppServerCodexHomeEnv::new(
		PathBuf::from("/Users/test/.codex"),
		PathBuf::from("/Users/test/.codex"),
	)
	.expect("test Codex home should validate");
	let response = InitializeResponse {
		user_agent: String::from("codex-cli-test"),
		codex_home: String::from("/Users/test/.codex"),
		platform_family: String::from("unix"),
		platform_os: String::from("macos"),
	};

	super::validate_initialize_codex_home(&expected, &response)
		.expect("matching Codex home should pass");
}

#[test]
fn initialize_codex_home_assertion_blocks_before_thread_start_on_mismatch() {
	let expected = ResolvedAppServerCodexHomeEnv::new(
		PathBuf::from("/Users/test/.codex"),
		PathBuf::from("/Users/test/.codex"),
	)
	.expect("test Codex home should validate");
	let response = InitializeResponse {
		user_agent: String::from("codex-cli-test"),
		codex_home: String::from("/tmp/per-account-codex-home"),
		platform_family: String::from("unix"),
		platform_os: String::from("macos"),
	};
	let error = super::validate_initialize_codex_home(&expected, &response)
		.expect_err("mismatched Codex home should fail before thread start");

	assert!(error.downcast_ref::<AppServerHomePreflightFailure>().is_some());
	assert!(error.to_string().contains("initialize codexHome `/tmp/per-account-codex-home`"));
	assert!(error.to_string().contains("expected shared Codex home `/Users/test/.codex`"));
	assert!(error.to_string().contains("before thread/start"));
}
