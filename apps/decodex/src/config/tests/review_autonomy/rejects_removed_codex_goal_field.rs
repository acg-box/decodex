use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn rejects_removed_codex_goal_field() {
	let removed_field = ["goal", "support"].join("_");

	for removed_value in ["auto", "required", "off"] {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = tests::write_config_file(
			temp_dir.path(),
			&format!(
				r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[codex]
				{removed_field} = "{removed_value}"
			"#
			),
		);
		let error = ServiceConfig::from_path(&config_path)
			.expect_err("removed goal field should be rejected");

		assert!(
			error.to_string().contains(&removed_field),
			"unexpected error for removed value `{removed_value}`: {error:?}"
		);
	}
}
