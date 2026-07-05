use std::fs;

use tempfile::TempDir;

use crate::agent::codex_accounts::{
	AccountPoolRecord, CodexAccountPool, CodexAccountProvider, CodexTokenData,
	DEFAULT_REFRESH_ENDPOINT, ProactiveRefreshReason,
};

#[test]
fn fixed_account_selection_uses_configured_account_without_balancing() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let usage_endpoint = super::start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":85},"secondary_window":{"used_percent":90}}}"#,
	]);
	let reset_credits_endpoint = super::start_codex_reset_credits_fixture_server(1);

	fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
{"email":"copy@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-copy","refresh_token":"refresh-copy","account_id":"acct_copy"}}
"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		reset_credits_endpoint,
		DEFAULT_REFRESH_ENDPOINT,
		Some("copy@example.com"),
	)
	.expect("account pool should initialize");
	let account = pool.select_account().expect("fixed account should select");

	assert_eq!(account.account_id(), "acct_copy");
	assert_eq!(account.summary().email.as_deref(), Some("copy@example.com"));
	assert_eq!(account.account_summaries().len(), 1);
}

#[test]
fn selection_marks_refresh_auth_failure_and_selects_next_account() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let refresh_endpoint = super::start_codex_refresh_status_fixture_server(vec![(
		401,
		"Unauthorized",
		r#"{"error":"invalid refresh token"}"#,
	)]);
	let usage_endpoint = super::start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
	]);
	let reset_credits_endpoint = super::start_codex_reset_credits_fixture_server(1);

	fs::write(
		&accounts_path,
		r#"{"email":"bad@example.com","auth_mode":"chatgpt","tokens":{"access_token":"x.eyJleHAiOjEwMDB9.y","refresh_token":"refresh-bad","account_id":"acct_bad"}}
{"email":"good@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-good","refresh_token":"refresh-good","account_id":"acct_good"}}
"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		reset_credits_endpoint,
		refresh_endpoint,
		None,
	)
	.expect("account pool should initialize");
	let account = pool.select_account().expect("healthy fallback account should select");

	assert_eq!(account.account_id(), "acct_good");

	let records = fs::read_to_string(&accounts_path).expect("accounts should read");

	assert!(records.contains(r#""auth_failed_at_unix_epoch":"#));
	assert!(records.contains("HTTP 401 Unauthorized"));

	let summaries = pool.account_activity_summaries_snapshot().expect("snapshot should load");
	let bad_summary = summaries
		.iter()
		.find(|summary| summary.email.as_deref() == Some("bad@example.com"))
		.expect("bad account summary should exist");

	assert_eq!(bad_summary.status, "auth_failed");
	assert_eq!(bad_summary.refresh_status, "auth_failed");
	assert!(bad_summary.note.as_deref().is_some_and(|note| note.contains("HTTP 401")));
}

#[test]
fn proactive_refresh_prefers_access_token_expiration_then_last_refresh() {
	let mut record = AccountPoolRecord {
		email: Some(String::from("refresh@example.com")),
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
			access_token: String::from("x.eyJleHAiOjEwMDB9.y"),
			refresh_token: String::from("refresh"),
			account_id: Some(String::from("acct_refresh")),
		}),
		last_refresh: Some(String::from("2099-01-01T00:00:00Z")),
	};

	assert_eq!(
		record.proactive_refresh_reason(1_001),
		Some(ProactiveRefreshReason::AccessTokenExpired)
	);

	record.tokens.as_mut().expect("tokens should exist").access_token = String::from("opaque");
	record.last_refresh = Some(String::from("2026-01-01T00:00:00Z"));

	assert_eq!(
		record.proactive_refresh_reason(1_768_000_000),
		Some(ProactiveRefreshReason::LastRefreshStale)
	);
}
