use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

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
