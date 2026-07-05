use std::fs;

use tempfile::TempDir;

use crate::config::ServiceConfig;

#[test]
fn loads_service_config_from_external_project_file_with_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");
	let config_dir = temp_dir.path().join("codex/decodex/projects/rsnap");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&config_dir).expect("config dir should exist");
	fs::write(
		&config_path,
		r#"
				service_id = "rsnap"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[paths]
				repo_root = "../../../../target-repo"
				worktree_root = "lanes"
			"#,
	)
	.expect("centralized config should write");

	let config = ServiceConfig::from_path(&config_path).expect("centralized config should load");
	let canonical_root = fs::canonicalize(&repo_root).expect("repo root should canonicalize");

	assert_eq!(config.service_id(), "rsnap");
	assert_eq!(config.repo_root(), canonical_root);
	assert_eq!(config.worktree_root(), canonical_root.join("lanes"));
	assert_eq!(
		config.workflow_path(),
		fs::canonicalize(&config_dir).expect("config dir should canonicalize").join("WORKFLOW.md")
	);
}
