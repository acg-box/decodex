use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn rejects_unknown_codex_review_level() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[codex]
				review = "prompt_only"
			"#,
	);
	let error =
		ServiceConfig::from_path(&config_path).expect_err("unknown review level should fail");

	assert!(error.to_string().contains("prompt_only"));
}
