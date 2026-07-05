use tempfile::TempDir;

use crate::{
	accounts::{store::AccountStore, tests},
	state::CodexAccountActivitySummary,
};

#[test]
fn usage_summary_marks_refresh_401_as_login_recovery() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[tests::account_record(
			"copy@example.com",
			"acct_123456",
			"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
			"refresh-secret",
		)])
		.expect("records should save");

	let mut response = store.list().expect("account list should load");

	response.apply_usage_summaries(&[CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		status: String::from("unusable"),
		refresh_status: String::from("failed"),
		note: Some(String::from(
			"usage probe failed: Codex account `copy@example.com` token refresh failed with HTTP 401 Unauthorized.",
		)),
		..CodexAccountActivitySummary::default()
	}]);

	assert_eq!(response.accounts[0].recovery_action.as_deref(), Some("login"));
}
