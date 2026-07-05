use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn loads_service_config_from_project_directory() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
	);
	let config = ServiceConfig::from_path(temp_dir.path())
		.expect("service config should load from project directory");

	assert_eq!(config.service_id(), "pubfi");
	assert_eq!(
		ServiceConfig::resolve_project_config_path(temp_dir.path())
			.expect("project directory should resolve"),
		config_path
	);
}
