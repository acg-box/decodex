use std::fs;

use tempfile::TempDir;

use crate::config::ServiceConfig;

#[test]
fn external_project_config_requires_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = temp_dir.path().join("project.toml");

	fs::write(
		&config_path,
		r#"
				service_id = "rsnap"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
	)
	.expect("centralized config should write");

	let error = ServiceConfig::from_path(&config_path).expect_err("repo_root should be required");

	assert!(
		error.to_string().contains("paths.repo_root"),
		"error should explain the missing explicit repo root: {error:?}"
	);
}
