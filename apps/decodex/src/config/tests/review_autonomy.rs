use tempfile::TempDir;

use crate::config::{ReviewLevel, ServiceConfig, tests};

#[test]
fn parses_codex_review_levels() {
	for (case_name, codex_body, expected_level) in [
		("default strict level", "", ReviewLevel::Strict),
		("explicit off level", r#"review = "off""#, ReviewLevel::Off),
		("explicit basic level", r#"review = "basic""#, ReviewLevel::Basic),
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

				[github]
				token_env_var = "HOME"

				[codex]
				{codex_body}
			"#
			),
		);
		let config = ServiceConfig::from_path(&config_path).expect(case_name);

		assert_eq!(config.codex().review_level(), expected_level);
	}
}

#[test]
fn parses_autonomy_objective_and_policy_references() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[autonomy]
				auto_promote = true
				auto_intake = true

				[autonomy.runtime_policy]
				accepted_objective_id = "quality-autonomy"
				accepted_objective_version = "1"
				accepted_policy_id = "pubfi-autonomy-policy"
				accepted_policy_version = "7"
				policy_authority_ref = "decodex.runtime_policy:pubfi-autonomy-policy@7"
				team_issue_identifier = "PUB-1000"
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");
	let autonomy = config.autonomy();
	let runtime_policy = autonomy.runtime_policy().expect("runtime policy references should parse");

	assert!(autonomy.auto_promote());
	assert!(autonomy.auto_intake());
	assert_eq!(runtime_policy.accepted_objective_id(), "quality-autonomy");
	assert_eq!(runtime_policy.accepted_objective_version(), "1");
	assert_eq!(runtime_policy.accepted_policy_id(), "pubfi-autonomy-policy");
	assert_eq!(runtime_policy.accepted_policy_version(), "7");
	assert_eq!(
		runtime_policy.policy_authority_ref(),
		"decodex.runtime_policy:pubfi-autonomy-policy@7"
	);
	assert_eq!(runtime_policy.team_issue_identifier(), Some("PUB-1000"));
}

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

#[test]
fn rejects_autonomy_execution_flags_without_required_authority_references() {
	for (case_name, autonomy_body, expected_error) in [
		(
			"auto promote needs runtime policy refs",
			r#"
				[autonomy]
				auto_promote = true
				"#,
			"runtime_policy",
		),
		(
			"auto intake needs auto promote",
			r#"
				[autonomy]
				auto_intake = true
				"#,
			"auto_promote",
		),
		(
			"auto intake needs tracker anchor",
			r#"
				[autonomy]
				auto_promote = true
				auto_intake = true

				[autonomy.runtime_policy]
				accepted_objective_id = "quality-autonomy"
				accepted_objective_version = "1"
				accepted_policy_id = "pubfi-autonomy-policy"
				accepted_policy_version = "7"
				policy_authority_ref = "decodex.runtime_policy:pubfi-autonomy-policy@7"
				"#,
			"team_issue_identifier",
		),
	] {
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

				{autonomy_body}
			"#
			),
		);
		let error = ServiceConfig::from_path(&config_path).expect_err(case_name);

		assert!(
			error.to_string().contains(expected_error),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
}

#[test]
fn rejects_autonomy_embedded_policy_bodies_and_execution_budgets() {
	for removed_field in [
		"objective_body",
		"policy_body",
		"allowed_signal_kinds",
		"allowed_surfaces",
		"validation_gates",
		"cooldown_seconds",
		"write_budget",
	] {
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

				[autonomy]
				auto_promote = false

				[autonomy.runtime_policy]
				accepted_objective_id = "quality-autonomy"
				accepted_objective_version = "1"
				accepted_policy_id = "pubfi-autonomy-policy"
				accepted_policy_version = "7"
				policy_authority_ref = "decodex.runtime_policy:pubfi-autonomy-policy@7"
				{removed_field} = "must-live-in-runtime-authority"
			"#
			),
		);
		let error = ServiceConfig::from_path(&config_path)
			.expect_err("embedded autonomy authority should be rejected");

		assert!(
			error.to_string().contains(removed_field),
			"error should identify rejected field {removed_field}: {error:?}"
		);
	}
}

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
