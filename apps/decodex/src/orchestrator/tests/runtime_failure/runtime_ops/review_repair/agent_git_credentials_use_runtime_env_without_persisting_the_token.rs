use crate::orchestrator::tests::{
	self,
	runtime_failure::{self, TestEnvVarGuard, fs, orchestrator, process},
};

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
