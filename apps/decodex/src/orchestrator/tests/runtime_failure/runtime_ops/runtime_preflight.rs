use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		self, RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE,
		RepoGateFailureKind, Report, TempDir, TestEnvVarGuard, fs, orchestrator, process,
	},
};

#[test]
fn repo_gate_runtime_failures_require_manual_attention_without_retry_budget_wait() {
	let error = Report::new(orchestrator::RepoGateFailure::new(
		RepoGateFailureKind::CommandSpawnFailed,
		String::from(
			"Failed to spawn repo gate command `cargo make fmt` in `/tmp/repo` via `/bin/sh` `-c`: missing tool",
		),
	));
	let repo_gate_failure = error
		.downcast_ref::<orchestrator::RepoGateFailure>()
		.expect("repo gate failure should downcast");

	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
}

#[test]
fn operation_marker_write_failures_do_not_abort_completion_flow() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let occupied_path = temp_dir.path().join("occupied");

	fs::write(&occupied_path, "not a directory").expect("blocking file should write");
	orchestrator::write_run_operation_marker_best_effort(
		&occupied_path,
		"run-1",
		1,
		RUN_OPERATION_REPO_GATE,
	);
	orchestrator::write_run_operation_marker_best_effort(
		&occupied_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	);

	assert!(occupied_path.is_file());
	assert!(!occupied_path.join(RUN_ACTIVITY_MARKER_FILE).exists());
}

#[test]
fn validate_review_handoff_runtime_requires_gh_and_github_token_authority() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();

	{
		let _env_lock = TestEnvVarGuard::lock();
		let missing_env_var = format!("DECODEX_TEST_MISSING_GITHUB_TOKEN_ENV_{}", process::id());
		let config_missing_github =
			runtime_failure::service_config_with_github_token_env_var(&config, &missing_env_var);

		assert!(orchestrator::validate_review_handoff_runtime(&config, true).is_ok());
		assert!(orchestrator::validate_review_handoff_runtime(&config, false).is_ok());
		assert!(orchestrator::validate_daemon_runtime().is_ok());
		assert!(orchestrator::validate_command_available("git", None, "test preflight").is_ok());

		let error = orchestrator::validate_review_handoff_runtime(&config_missing_github, false)
			.expect_err("missing github token env-var should fail live preflight");

		assert!(error.to_string().contains("github.token_env_var"));
	}

	let env_var = format!("DECODEX_TEST_BLANK_GITHUB_TOKEN_ENV_{}", process::id());
	let _env_guard = TestEnvVarGuard::set(&env_var, "");
	let config_blank_github =
		runtime_failure::service_config_with_github_token_env_var(&config, &env_var);
	let error = orchestrator::validate_review_handoff_runtime(&config_blank_github, false)
		.expect_err("blank github token authority should fail live preflight");

	assert!(error.to_string().contains("must not be blank"));

	let error = orchestrator::validate_command_available(
		"__decodex_missing_command__",
		None,
		"PR-backed review handoff",
	)
	.expect_err("missing command should fail preflight");

	assert!(
		error.to_string().contains("Required command `__decodex_missing_command__` is unavailable")
	);
}
