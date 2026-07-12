use std::fs;

use tempfile::TempDir;

use crate::config::{ReviewLevel, ServiceConfig, tests};

#[test]
fn loads_service_config_from_project_file_with_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"
team_id = "team-test"

				[github]
				token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"
				command_path = "bin/gh"
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");
	let canonical_root = fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

	assert_eq!(config.service_id(), "pubfi");
	assert_eq!(config.repo_root(), canonical_root);
	assert_eq!(config.worktree_root(), canonical_root.join(".worktrees"));
	assert_eq!(config.workflow_path(), canonical_root.join("WORKFLOW.md"));
	assert_eq!(config.tracker().team_id(), "team-test");
	assert_eq!(config.github().token_env_var(), "HOME");
	assert_eq!(config.github().owner(), "test-owner");
	assert_eq!(config.github().repository(), "test-repository");
	assert_eq!(config.github().command_path(), Some(canonical_root.join("bin/gh").as_path()));
	let binding = config.project_binding("fingerprint");
	assert_eq!(binding.project_key(), "pubfi");
	assert_eq!(binding.tracker_team_id(), "team-test");
	assert_eq!(binding.routing_label(), "decodex:queued:pubfi");
	assert_eq!(binding.config_fingerprint(), "fingerprint");
	assert_eq!(config.codex().review_level(), ReviewLevel::Strict);
}
