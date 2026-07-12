use std::fs;

use tempfile::TempDir;

#[rustfmt::skip]
use crate::manual::{self, tests};
use crate::config::ServiceConfig;

#[test]
fn ensure_cli_repo_context_rejects_foreign_config_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let current_repo = tests::init_git_checkout(&temp_dir, "current-repo");
	let foreign_repo = tests::init_git_checkout(&temp_dir, "foreign-repo");
	let config_path = foreign_repo.join("project.toml");

	fs::write(
		&config_path,
		r#"
			service_id = "pubfi"

			[tracker]
			api_key_env_var = "HOME"
team_id = "team-test"

			[github]
			token_env_var = "PATH"
owner = "test-owner"
repository = "test-repository"

			[paths]
			repo_root = "."
			"#,
	)
	.expect("foreign config should write");

	let config = ServiceConfig::from_path(&config_path).expect("config should parse");
	let canonical_repo_root =
		fs::canonicalize(&current_repo).expect("current repo root should canonicalize");
	let error = manual::ensure_cli_repo_context(&current_repo, &config, &canonical_repo_root)
		.expect_err("foreign config repo root should be rejected");

	assert!(error.to_string().contains("does not match loaded config repo root"));
	assert!(error.to_string().contains(&foreign_repo.display().to_string()));
}
