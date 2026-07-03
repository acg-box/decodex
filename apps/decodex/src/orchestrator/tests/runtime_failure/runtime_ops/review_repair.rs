use crate::{
	orchestrator::tests::{
		self,
		runtime_failure::{
			self, AgentGitCredentialsUnavailable, RetainedReviewRepairPushFailed,
			RunFailureWritebackDisposition, TestEnvVarGuard, fs, orchestrator, process,
		},
	},
	test_support,
};

#[test]
fn retained_review_repair_completion_pushes_repaired_head_to_pr_branch() {
	let (temp_dir, base_config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_REPAIR_PUSH_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&base_config, &env_var);
	let remote_root = temp_dir.path().join("origin.git");
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 2);

	runtime_failure::add_origin_remote(config.repo_root(), &remote_root);
	runtime_failure::checkout_new_branch(config.repo_root(), &issue_run.worktree.branch_name);

	let local_head = runtime_failure::commit_worktree_change(
		config.repo_root(),
		"repair.txt",
		"repair\n",
		r#"{"schema":"decodex/commit/1","summary":"Retain review repair","authority":"XY-1115"}"#,
	);

	orchestrator::push_retained_review_repair_head(
		&config,
		&issue_run,
		Some("https://github.com/hack-ink/decodex/pull/502"),
	)
	.expect("retained review-repair completion should push the repaired head");

	let output = test_support::hermetic_git_command()
		.arg("--git-dir")
		.arg(&remote_root)
		.args(["rev-parse", &format!("refs/heads/{}", issue_run.worktree.branch_name)])
		.output()
		.expect("remote head probe should run");

	assert!(
		output.status.success(),
		"remote retained repair branch should exist: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), local_head);
}

#[test]
fn retained_review_repair_push_failures_are_structured_terminal_attention() {
	let _env_lock = TestEnvVarGuard::lock();
	let (_temp_dir, base_config, _workflow) = tests::temp_project_layout();
	let missing_env_var = format!("DECODEX_TEST_MISSING_REPAIR_PUSH_TOKEN_ENV_{}", process::id());
	let config =
		runtime_failure::service_config_with_github_token_env_var(&base_config, &missing_env_var);
	let issue = tests::sample_issue("In Review", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 2);
	let error = orchestrator::push_retained_review_repair_head(
		&config,
		&issue_run,
		Some("https://github.com/hack-ink/decodex/pull/502"),
	)
	.expect_err("missing GitHub token should produce a typed push failure");
	let push_failure = error
		.downcast_ref::<RetainedReviewRepairPushFailed>()
		.expect("missing push authority should preserve typed error");
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`",
	);

	assert_eq!(push_failure.error_class(), "retained_review_repair_push_auth_failed");
	assert_eq!(error_class, "retained_review_repair_push_auth_failed");
	assert!(next_action.contains("repair GitHub authentication"));
	assert!(next_action.contains(&issue_run.worktree.branch_name));
	assert_eq!(
		orchestrator::run_failure_writeback_disposition(&error),
		RunFailureWritebackDisposition::TerminalAttention
	);
}

#[test]
fn agent_git_credentials_use_runtime_env_without_persisting_the_token() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_AGENT_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&config, &env_var);
	let credentials =
		orchestrator::prepare_agent_git_credentials(&config, "run/with spaces", config.repo_root())
			.expect("agent Git credentials should prepare");

	assert!(
		fs::read_dir(config.worktree_root())
			.expect("worktree root should list")
			.filter_map(std::result::Result::ok)
			.all(|entry| !entry.file_name().to_string_lossy().starts_with(".decodex-git-askpass-")),
		"agent Git credentials should not materialize askpass helper files"
	);

	let inherited_signing_key =
		runtime_failure::git_config_value(config.repo_root(), "user.signingkey", None);
	let agent_signing_key = runtime_failure::git_config_value(
		config.repo_root(),
		"user.signingkey",
		Some(&credentials),
	);

	assert_eq!(
		agent_signing_key, inherited_signing_key,
		"agent git environment should preserve inherited signing keys when the repo has no local key"
	);
	assert_eq!(
		runtime_failure::git_config_value(config.repo_root(), "commit.gpgsign", Some(&credentials))
			.as_deref(),
		Some("false")
	);

	let inherited_git_config_keys = runtime_failure::injected_git_config_keys(&credentials);

	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "commit.gpgsign"),
		"agent git environment should not disable inherited commit signing"
	);
	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "tag.gpgsign"),
		"agent git environment should not disable inherited tag signing"
	);
	assert!(
		!inherited_git_config_keys.iter().any(|key| key == "user.signingkey"),
		"agent git environment should not mask inherited signing keys"
	);

	let injected_git_config_values = runtime_failure::injected_git_config_values(&credentials);

	assert!(
		injected_git_config_values
			.iter()
			.any(|value| value.contains("github.com") && value.contains("x-access-token")),
		"agent git environment should inject an inline GitHub credential helper"
	);
	assert!(
		!injected_git_config_values.iter().any(|value| value.contains("secret-token-value")),
		"agent git config should not persist the GitHub token"
	);
}

#[test]
fn agent_git_credentials_pin_repo_local_signing_key_when_configured() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let env_var = format!("DECODEX_TEST_AGENT_SIGNING_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "secret-token-value");
	let config = runtime_failure::service_config_with_github_token_env_var(&config, &env_var);

	runtime_failure::git_status_success(
		config.repo_root(),
		&["config", "user.signingkey", "route-y-signing-key"],
	);

	let credentials = orchestrator::prepare_agent_git_credentials(
		&config,
		"run-with-signing",
		config.repo_root(),
	)
	.expect("agent Git credentials should prepare");
	let mut signing_key_probe = test_support::hermetic_git_command();

	signing_key_probe.arg("-C").arg(config.repo_root()).args([
		"config",
		"--get",
		"user.signingkey",
	]);
	credentials.process_env().apply_to(&mut signing_key_probe).expect("agent env should apply");

	let output = signing_key_probe.output().expect("git signing key probe should run");

	assert!(output.status.success());
	assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "route-y-signing-key");
}

#[test]
fn missing_agent_git_credentials_stop_without_retry() {
	let _env_lock = TestEnvVarGuard::lock();
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let missing_env_var = format!("DECODEX_TEST_MISSING_AGENT_GITHUB_TOKEN_ENV_{}", process::id());
	let config =
		runtime_failure::service_config_with_github_token_env_var(&config, &missing_env_var);
	let error = match orchestrator::prepare_agent_git_credentials(
		&config,
		"run-missing-token",
		config.repo_root(),
	) {
		Ok(_) => panic!("missing github token should fail before app-server launch"),
		Err(error) => error,
	};
	let credentials_error = error
		.downcast_ref::<AgentGitCredentialsUnavailable>()
		.expect("credential preflight failure should be typed");
	let (error_class, next_action) = orchestrator::terminal_failure_comment_details(
		false,
		&error,
		"clear label `decodex:needs-attention`",
	);

	assert_eq!(credentials_error.token_env_var, missing_env_var);
	assert_eq!(error_class, "github_credentials_unavailable");
	assert!(next_action.contains("repair GitHub authentication"));
	assert!(!next_action.contains(&missing_env_var));
}
