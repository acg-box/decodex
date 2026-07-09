use tempfile::TempDir;

use crate::config::{FAST_LANDING_STATUS_CONTEXT, ProjectGitHubLandingMode, ServiceConfig, tests};

#[test]
fn github_landing_defaults_to_standard() {
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
	let config = ServiceConfig::from_path(&config_path).expect("config should parse");

	assert_eq!(config.github().landing_mode(), ProjectGitHubLandingMode::Standard);
	assert!(config.github().landing_actors().is_empty());
	assert!(config.github().landing_status_contexts().is_empty());
}

#[test]
fn parses_github_fast_landing_mode_and_actors() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
		service_id = "pubfi"

		[tracker]
		api_key_env_var = "HOME"

		[github]
		token_env_var = "HOME"
		landing_mode = "fast"
		landing_actors = ["aurexav", "yvette-carlisle"]
	"#,
	);
	let config = ServiceConfig::from_path(&config_path).expect("config should parse");

	assert_eq!(config.github().landing_mode(), ProjectGitHubLandingMode::Fast);
	assert_eq!(
		config.github().landing_status_contexts(),
		&[String::from(FAST_LANDING_STATUS_CONTEXT)]
	);
	assert_eq!(
		config.github().landing_actors(),
		&[String::from("aurexav"), String::from("yvette-carlisle")]
	);
}

#[test]
fn fast_landing_requires_at_least_one_actor() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
		service_id = "pubfi"

		[tracker]
		api_key_env_var = "HOME"

		[github]
		token_env_var = "HOME"
		landing_mode = "fast"
	"#,
	);
	let error = ServiceConfig::from_path(&config_path)
		.expect_err("fast landing without actors should fail");

	assert!(error.to_string().contains("github.landing_actors"));
}

#[test]
fn standard_landing_rejects_actors() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
		service_id = "pubfi"

		[tracker]
		api_key_env_var = "HOME"

		[github]
		token_env_var = "HOME"
		landing_actors = ["aurexav"]
	"#,
	);
	let error = ServiceConfig::from_path(&config_path)
		.expect_err("standard landing with actors should fail");

	assert!(error.to_string().contains("github.landing_actors"));
}
