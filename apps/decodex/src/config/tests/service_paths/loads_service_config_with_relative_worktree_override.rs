use std::fs;

use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn loads_service_config_with_relative_worktree_override() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[paths]
				worktree_root = "var/worktrees"
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");
	let canonical_root = fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

	assert_eq!(config.worktree_root(), canonical_root.join("var/worktrees"));
}
