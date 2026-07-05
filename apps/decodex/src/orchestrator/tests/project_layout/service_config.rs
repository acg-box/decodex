use crate::orchestrator::tests::{
	Path, PathBuf, ReviewLevel, ServiceConfig, TEST_PROJECT_CONFIG_FILE, fs,
};

pub(in crate::orchestrator::tests) fn write_service_config(repo_root: &Path, contents: &str) {
	fs::create_dir_all(service_config_dir(repo_root)).expect("service config dir should exist");

	let contents =
		contents.replace("repo_root = \".\"", &format!("repo_root = \"{}\"", repo_root.display()));

	fs::write(service_config_path(repo_root), contents).expect("service config should write");
}

pub(in crate::orchestrator::tests) fn load_service_config(repo_root: &Path) -> ServiceConfig {
	ServiceConfig::from_path(service_config_path(repo_root)).expect("service config should load")
}

pub(in crate::orchestrator::tests) fn service_config_path(repo_root: &Path) -> PathBuf {
	service_config_dir(repo_root).join(TEST_PROJECT_CONFIG_FILE)
}

pub(in crate::orchestrator::tests) fn service_config_dir(repo_root: &Path) -> PathBuf {
	repo_root
		.parent()
		.expect("repo root should have temp parent")
		.join(".codex/decodex/projects/project")
}

pub(in crate::orchestrator::tests) fn service_workflow_path(repo_root: &Path) -> PathBuf {
	service_config_dir(repo_root).join("WORKFLOW.md")
}

pub(in crate::orchestrator::tests) fn sample_service_config_toml(
	service_id: &str,
	tracker_api_key_env_var: &str,
	github_token_env_var: &str,
	worktree_root: Option<&Path>,
	review_level: ReviewLevel,
) -> String {
	sample_service_config_toml_with_github_command_path(
		service_id,
		tracker_api_key_env_var,
		github_token_env_var,
		worktree_root,
		review_level,
		None,
	)
}

pub(in crate::orchestrator::tests) fn sample_service_config_toml_with_github_command_path(
	service_id: &str,
	tracker_api_key_env_var: &str,
	github_token_env_var: &str,
	worktree_root: Option<&Path>,
	review_level: ReviewLevel,
	github_command_path: Option<&Path>,
) -> String {
	let mut toml = format!(
		r#"service_id = "{service_id}"

[tracker]
api_key_env_var = "{tracker_api_key_env_var}"

[github]
token_env_var = "{github_token_env_var}"
"#
	);

	if let Some(github_command_path) = github_command_path {
		toml.push_str(&format!("command_path = \"{}\"\n", github_command_path.display()));
	}

	if review_level != ReviewLevel::Strict {
		toml.push_str("\n\n[codex]\n");
		toml.push_str(&format!("review = \"{}\"\n", review_level.as_str()));
	}

	toml.push_str(
		r#"

[paths]
repo_root = "."
"#,
	);

	if let Some(worktree_root) = worktree_root {
		toml.push_str(&format!("worktree_root = \"{}\"\n", worktree_root.display()));
	}

	toml
}

pub(in crate::orchestrator::tests) fn service_config_toml_for_config(
	config: &ServiceConfig,
	github_token_env_var: &str,
	review_level: ReviewLevel,
) -> String {
	service_config_toml_for_config_with_github_command_path(
		config,
		github_token_env_var,
		review_level,
		config.github().command_path(),
	)
}

pub(in crate::orchestrator::tests) fn service_config_toml_for_config_with_github_command_path(
	config: &ServiceConfig,
	github_token_env_var: &str,
	review_level: ReviewLevel,
	github_command_path: Option<&Path>,
) -> String {
	let default_worktree_root = config.repo_root().join(".worktrees");
	let worktree_root =
		(config.worktree_root() != default_worktree_root).then_some(config.worktree_root());

	sample_service_config_toml_with_github_command_path(
		config.service_id(),
		config.tracker().api_key_env_var(),
		github_token_env_var,
		worktree_root,
		review_level,
		github_command_path,
	)
}

pub(in crate::orchestrator::tests) fn service_config_with_github_token_env_var(
	config: &ServiceConfig,
	token_env_var: &str,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config(config, token_env_var, config.codex().review_level()),
	);

	load_service_config(config.repo_root())
}

pub(in crate::orchestrator::tests) fn service_config_with_github_token_env_var_and_command_path(
	config: &ServiceConfig,
	token_env_var: &str,
	github_command_path: &Path,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config_with_github_command_path(
			config,
			token_env_var,
			config.codex().review_level(),
			Some(github_command_path),
		),
	);

	load_service_config(config.repo_root())
}

pub(in crate::orchestrator::tests) fn service_config_with_review_level(
	config: &ServiceConfig,
	review_level: ReviewLevel,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config_with_github_command_path(
			config,
			config.github().token_env_var(),
			review_level,
			config.github().command_path(),
		),
	);

	load_service_config(config.repo_root())
}
