use std::fs;

use tempfile::TempDir;

use crate::agent::codex_accounts::{
	AccountPoolRecord, CodexAccountPool, CodexTokenData, DEFAULT_REFRESH_ENDPOINT, Path, record,
};

#[test]
fn accounts_accept_flat_and_wrapped_auth_jsonl_records() {
	let input = r#"
		{"email":"primary@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id","access_token":"access","refresh_token":"refresh","account_id":"acct_primary"}}
		{"auth":{"auth_mode":"chatgpt","tokens":{"id_token":"x.eyJlbWFpbCI6IndyYXBwZWRAZXhhbXBsZS5jb20ifQ.y","access_token":"access-2","refresh_token":"refresh-2","account_id":"acct_wrapped"}}}
	"#;
	let records = record::parse_account_records(input, Path::new("/tmp/accounts.jsonl"))
		.expect("records should parse");

	assert_eq!(records.len(), 2);
	assert_eq!(records[0].account_id(), Some("acct_primary"));
	assert_eq!(records[0].email().as_deref(), Some("primary@example.com"));
	assert_eq!(records[1].account_id(), Some("acct_wrapped"));
	assert_eq!(records[1].email().as_deref(), Some("wrapped@example.com"));
}

#[test]
fn account_selector_matches_email_full_id_and_fingerprint() {
	let record = AccountPoolRecord {
		email: Some(String::from("selected@example.com")),
		disabled: false,
		cooldown_until_unix_epoch: None,
		cooldown_until: None,
		last_selected_at_unix_epoch: None,
		auth_failed_at_unix_epoch: None,
		auth_failure: None,
		auth_mode: Some(String::from("chatgpt")),
		openai_api_key: None,
		tokens: Some(CodexTokenData {
			email: None,
			id_token: None,
			access_token: String::from("access"),
			refresh_token: String::from("refresh"),
			account_id: Some(String::from("acct_fixed_123456")),
		}),
		last_refresh: None,
	};

	assert!(record.matches_account_selector("selected@example.com"));
	assert!(record.matches_account_selector("acct_fixed_123456"));
	assert!(record.matches_account_selector("...123456"));
	assert!(!record.matches_account_selector("other@example.com"));
}

#[test]
fn account_activity_snapshot_uses_configured_records_without_usage_probe() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");

	fs::write(
		&accounts_path,
		r#"{"email":"snapshot@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-snapshot","refresh_token":"refresh-snapshot","account_id":"acct_snapshot"}}"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		"http://127.0.0.1:9/usage",
		DEFAULT_REFRESH_ENDPOINT,
		None,
	)
	.expect("account pool should initialize");
	let summaries = pool.account_activity_summaries_snapshot().expect("snapshot should load");

	assert_eq!(summaries.len(), 1);
	assert_eq!(summaries[0].email.as_deref(), Some("snapshot@example.com"));
	assert_eq!(summaries[0].status, "available");
	assert_eq!(summaries[0].refresh_status, "not_checked");
	assert_eq!(summaries[0].primary_remaining_percent, None);
	assert_eq!(summaries[0].note.as_deref(), Some("configured account"));
}
