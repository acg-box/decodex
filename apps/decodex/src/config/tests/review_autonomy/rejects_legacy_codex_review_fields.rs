use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn rejects_legacy_codex_review_fields() {
	for (removed_field, removed_value) in
		[("external_review_enabled", "false"), ("internal_review_mode", "\"prompt\"")]
	{
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
				{removed_field} = {removed_value}
			"#
			),
		);
		let error = ServiceConfig::from_path(&config_path)
			.expect_err("legacy codex review field should be rejected");

		assert!(
			error.to_string().contains(removed_field),
			"error should identify removed field {removed_field}: {error:?}"
		);
	}
}
