use tempfile::TempDir;

use crate::config::{ServiceConfig, tests};

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
