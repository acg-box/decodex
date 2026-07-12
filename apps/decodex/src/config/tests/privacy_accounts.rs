use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

#[test]
fn project_privacy_classifier_defaults_to_disabled() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"
team_id = "team-test"

				[github]
				token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");

	assert_eq!(config.privacy_classifier().endpoint(), None);
	assert_eq!(config.privacy_classifier().timeout_ms(), 1_000);
}

#[test]
fn parses_loopback_privacy_classifier_endpoint() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"
team_id = "team-test"

				[github]
				token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"

				[privacy_classifier]
				endpoint = "http://127.0.0.1:9123/classify"
				timeout_ms = 250
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");

	assert_eq!(config.privacy_classifier().endpoint(), Some("http://127.0.0.1:9123/classify"));
	assert_eq!(config.privacy_classifier().timeout_ms(), 250);
}

#[test]
fn rejects_remote_privacy_classifier_endpoint() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"
team_id = "team-test"

				[github]
				token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"

				[privacy_classifier]
				endpoint = "https://example.com/classify"
			"#,
	);
	let error = ServiceConfig::from_path(&config_path)
		.expect_err("remote classifier endpoints should be rejected");

	assert!(
		error.to_string().contains("loopback"),
		"error should explain local-only classifier routing: {error:?}"
	);
}

#[test]
fn parses_codex_accounts_settings() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
					api_key_env_var = "HOME"
team_id = "team-test"

					[github]
					token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"

						[codex.accounts]
						usage_endpoint = "http://127.0.0.1:1234/wham/usage"
						profile_endpoint = "http://127.0.0.1:1234/wham/profiles/me"
						reset_credits_endpoint = "http://127.0.0.1:1234/wham/rate-limit-reset-credits"
						refresh_endpoint = "http://127.0.0.1:1234/oauth/token"
					"#,
	);
	let config = ServiceConfig::from_path(&config_path).expect("accounts should parse");
	let accounts = config.codex().accounts().expect("accounts should be configured");

	assert_eq!(accounts.usage_endpoint(), Some("http://127.0.0.1:1234/wham/usage"));
	assert_eq!(accounts.profile_endpoint(), Some("http://127.0.0.1:1234/wham/profiles/me"));
	assert_eq!(
		accounts.reset_credits_endpoint(),
		Some("http://127.0.0.1:1234/wham/rate-limit-reset-credits")
	);
	assert_eq!(accounts.refresh_endpoint(), Some("http://127.0.0.1:1234/oauth/token"));
}

#[test]
fn rejects_removed_project_scoped_codex_account_fields() {
	for (case_name, removed_field) in [
		("project-scoped account selection", r#"fixed_account = "primary@example.com""#),
		("legacy account path override", r#"path = "accounts/codex-auth.jsonl""#),
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

					[codex.accounts]
					{removed_field}
				"#
			),
		);
		let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

		assert!(
			error.to_string().contains(
				removed_field
					.split_once(" = ")
					.expect("removed field assignment should include a separator")
					.0
			),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
