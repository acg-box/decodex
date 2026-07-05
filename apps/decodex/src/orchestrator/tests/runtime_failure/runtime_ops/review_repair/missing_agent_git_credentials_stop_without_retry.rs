use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		self, AgentGitCredentialsUnavailable, TestEnvVarGuard, orchestrator, process,
	},
};

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
