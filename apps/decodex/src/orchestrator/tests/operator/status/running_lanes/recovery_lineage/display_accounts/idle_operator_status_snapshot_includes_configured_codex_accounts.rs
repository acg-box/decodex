use std::fs;

use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, TestEnvVarGuard, orchestrator, recovery_lineage::usage_fixture,
};

#[test]
fn idle_operator_status_snapshot_includes_configured_codex_accounts() {
	let (temp_dir, base_config, _workflow) = running_lanes::temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let accounts_path = temp_dir.path().join(".codex/decodex/accounts.jsonl");
	let usage_endpoint = usage_fixture::start_codex_usage_fixture_server(vec![
		(
			"acct_default",
			r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":7,"limit_window_seconds":18000,"reset_at":1800018000},"secondary_window":{"used_percent":11,"limit_window_seconds":604800,"reset_at":1800604800}},"credits":{"has_credits":true,"unlimited":false,"balance":"12.34"}}"#,
		),
		(
			"acct_copy",
			r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":22,"limit_window_seconds":18000,"reset_at":1800019000},"secondary_window":{"used_percent":33,"limit_window_seconds":604800,"reset_at":1800605800}},"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}"#,
		),
	]);

	fs::create_dir_all(accounts_path.parent().expect("accounts path should have parent"))
		.expect("accounts dir should exist");
	fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
{"email":"copy@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-copy","refresh_token":"refresh-copy","account_id":"acct_copy"}}
"#,
	)
	.expect("accounts fixture should write");

	let mut config_toml = running_lanes::service_config_toml_for_config(
		&base_config,
		base_config.github().token_env_var(),
		base_config.codex().review_level(),
	);

	config_toml.push_str(&format!("\n[codex.accounts]\nusage_endpoint = \"{}\"\n", usage_endpoint));

	running_lanes::write_service_config(base_config.repo_root(), &config_toml);

	let config = running_lanes::load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot_json["account_control"]["mode"], "balanced");
	assert!(snapshot_json["account_control"]["account_selector"].is_null());
	assert_eq!(accounts.len(), 2);
	assert_eq!(accounts[0]["email"], "default@example.com");
	assert_eq!(accounts[0]["status"], "available");
	assert_eq!(accounts[0]["refresh_status"], "not_needed");
	assert_eq!(accounts[0]["plan_type"], "pro");
	assert_eq!(accounts[0]["primary_remaining_percent"], 93);
	assert_eq!(accounts[0]["credits_balance"], "12.34");
	assert_eq!(accounts[1]["email"], "copy@example.com");
	assert_eq!(accounts[1]["status"], "available");
	assert_eq!(accounts[1]["refresh_status"], "not_needed");
	assert_eq!(accounts[1]["plan_type"], "plus");
	assert_eq!(accounts[1]["primary_remaining_percent"], 78);
	assert_eq!(accounts[1]["credits_balance"], "0");
}
