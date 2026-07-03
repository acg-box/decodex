use tempfile::TempDir;

use crate::config::{
	ServiceConfig,
	tests::{self, TestEnvVarGuard},
};

#[test]
fn rejects_empty_github_token_env_var() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = ""
			"#,
	);
	let error = ServiceConfig::from_path(&config_path)
		.expect_err("empty github token env-var should be rejected");

	assert!(error.to_string().contains("github.token_env_var"));
}

#[test]
fn rejects_blank_secret_env_var_values_when_resolving() {
	#[derive(Clone, Copy)]
	enum SecretTarget {
		Github,
		Tracker,
	}

	for (case_name, env_var, env_value, target) in [
		(
			"empty github token env-var value",
			"DECODEX_TEST_EMPTY_GITHUB_TOKEN",
			"",
			SecretTarget::Github,
		),
		(
			"whitespace-only github token env-var value",
			"DECODEX_TEST_BLANK_GITHUB_TOKEN",
			"   ",
			SecretTarget::Github,
		),
		(
			"whitespace-only tracker api key env-var value",
			"DECODEX_TEST_BLANK_TRACKER_API_KEY",
			"   ",
			SecretTarget::Tracker,
		),
	] {
		let _guard = TestEnvVarGuard::set(env_var, env_value);
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = tests::write_config_file(
			temp_dir.path(),
			&format!(
				r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "{}"

				[github]
				token_env_var = "{}"
			"#,
				match target {
					SecretTarget::Github => "HOME",
					SecretTarget::Tracker => env_var,
				},
				match target {
					SecretTarget::Github => env_var,
					SecretTarget::Tracker => "HOME",
				},
			),
		);
		let config = ServiceConfig::from_path(&config_path).expect("service config should parse");
		let error = match target {
			SecretTarget::Github => config.github().resolve_token(),
			SecretTarget::Tracker => config.tracker().resolve_api_key(),
		}
		.expect_err(case_name);

		assert!(
			error.to_string().contains("must not be blank"),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn rejects_invalid_service_ids() {
	for (case_name, service_id, expected) in [
		("empty service_id", "", "service_id"),
		(
			"service_id with non-slug characters",
			"pub:fi",
			"lowercase ASCII letters, digits, hyphens, or underscores",
		),
	] {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let config_path = tests::write_config_file(
			temp_dir.path(),
			&format!(
				r#"
				service_id = "{service_id}"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#
			),
		);
		let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

		assert!(
			error.to_string().contains(expected),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}
