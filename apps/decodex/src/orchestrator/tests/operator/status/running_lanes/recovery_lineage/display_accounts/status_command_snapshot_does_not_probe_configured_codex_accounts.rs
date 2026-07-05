use std::fs;

use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, TestEnvVarGuard, orchestrator,
};

#[test]
fn status_command_snapshot_does_not_probe_configured_codex_accounts() {
	let (temp_dir, base_config, workflow) = running_lanes::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let accounts_path = temp_dir.path().join(".codex/decodex/accounts.jsonl");

	fs::create_dir_all(accounts_path.parent().expect("accounts path should have parent"))
		.expect("accounts dir should exist");
	fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
"#,
	)
	.expect("accounts fixture should write");

	let mut config_toml = running_lanes::service_config_toml_for_config(
		&base_config,
		base_config.github().token_env_var(),
		base_config.codex().review_level(),
	);

	config_toml
		.push_str("\n[codex.accounts]\nusage_endpoint = \"http://127.0.0.1:9/wham/usage\"\n");

	running_lanes::write_service_config(base_config.repo_root(), &config_toml);

	let config = running_lanes::load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_status_command_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status command snapshot should build without probing account usage");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert_eq!(accounts.len(), 1);
	assert_eq!(accounts[0]["email"], "default@example.com");
	assert_eq!(accounts[0]["status"], "available");
	assert_eq!(accounts[0]["refresh_status"], "not_checked");
	assert!(accounts[0]["primary_remaining_percent"].is_null());
	assert!(!snapshot.warnings.contains(&String::from("codex_accounts_unavailable")));
}
