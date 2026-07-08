use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn parses_github_landing_required_status_contexts() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
		service_id = "pubfi"

		[tracker]
		api_key_env_var = "HOME"

		[github]
		token_env_var = "HOME"
		landing_required_status_contexts = ["decodex/local-full-check"]
		landing_required_status_creators = ["decodex-bot"]
	"#,
	);
	let config = ServiceConfig::from_path(&config_path).expect("config should parse");

	assert_eq!(
		config.github().landing_required_status_contexts(),
		&[String::from("decodex/local-full-check")]
	);
	assert_eq!(config.github().landing_required_status_creators(), &[String::from("decodex-bot")]);
}
