use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

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
team_id = "team-test"

				[github]
				token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"

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
