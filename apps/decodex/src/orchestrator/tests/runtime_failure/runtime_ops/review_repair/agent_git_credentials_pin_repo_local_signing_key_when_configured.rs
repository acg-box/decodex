use crate::{
	orchestrator::tests::{
		self,
		runtime_failure::{self, TestEnvVarGuard, orchestrator, process},
	},
	test_support,
};

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
