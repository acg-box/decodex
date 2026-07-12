use tempfile::TempDir;

use crate::config::{ReviewLevel, ServiceConfig, tests};

#[test]
fn parses_codex_review_levels() {
	for (case_name, codex_body, expected_level) in [
		("default strict level", "", ReviewLevel::Strict),
		("explicit off level", r#"review = "off""#, ReviewLevel::Off),
		("explicit standard level", r#"review = "standard""#, ReviewLevel::Standard),
		("explicit strict level", r#"review = "strict""#, ReviewLevel::Strict),
	] {
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
				{codex_body}
			"#
			),
		);
		let config = ServiceConfig::from_path(&config_path).expect(case_name);

		assert_eq!(config.codex().review_level(), expected_level);
	}
}
