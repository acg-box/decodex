use std::fs;

use tempfile::TempDir;

use crate::config::ServiceConfig;

#[test]
fn rejects_project_config_with_nonstandard_file_name() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = temp_dir.path().join("rsnap.toml");

	fs::write(&config_path, "").expect("config should write");

	let error = ServiceConfig::from_path(&config_path)
		.expect_err("nonstandard config file name should fail");

	assert!(
		error.to_string().contains("project.toml"),
		"error should explain the fixed config file name: {error:?}"
	);
}
