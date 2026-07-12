use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn rejects_unknown_codex_review_level() {
	for (case_name, review_level) in
		[("unknown prompt-only level", "prompt_only"), ("removed basic level", "basic")]
	{
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = tests::write_config_file(
			temp_dir.path(),
			&format!(
				r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"
team_id = "team-test"

				[github]
				token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"

				[codex]
				review = "{review_level}"
			"#
			),
		);
		let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

		assert!(error.to_string().contains(review_level));
	}
}
