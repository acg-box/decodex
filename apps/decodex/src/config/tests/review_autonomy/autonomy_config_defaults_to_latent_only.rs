use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn autonomy_config_defaults_to_latent_only() {
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
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");

	assert!(!config.autonomy().auto_promote());
	assert!(!config.autonomy().auto_intake());
	assert!(config.autonomy().runtime_policy().is_none());
}
