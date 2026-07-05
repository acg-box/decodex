use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use crate::agent::codex_accounts::{
	CodexAccountAuthFailure, CodexAccountPool, CodexAccountProvider,
};

#[test]
fn refresh_account_marks_auth_failure_and_returns_typed_error() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let refresh_endpoint = super::start_codex_refresh_status_fixture_server(vec![(
		401,
		"Unauthorized",
		r#"{"error":"invalid refresh token"}"#,
	)]);
	let usage_endpoint = super::start_codex_usage_fixture_server(Vec::new());
	let reset_credits_endpoint = super::start_codex_reset_credits_fixture_server(0);

	fs::write(
		&accounts_path,
		r#"{"email":"bad@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-bad","refresh_token":"refresh-bad","account_id":"acct_bad"}}"#,
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
	let error = match pool.refresh_account(Some("acct_bad")) {
		Ok(_) => panic!("refresh auth failure should surface"),
		Err(error) => error,
	};

	assert!(error.downcast_ref::<CodexAccountAuthFailure>().is_some());

	let records = fs::read_to_string(&accounts_path).expect("accounts should read");

	assert!(records.contains(r#""auth_failed_at_unix_epoch":"#));
	assert!(records.contains("HTTP 401 Unauthorized"));
}

#[test]
fn token_refresh_syncs_matching_codex_auth_json() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let codex_auth_path = temp_dir.path().join(".codex/auth.json");
	let refresh_endpoint = super::start_codex_refresh_fixture_server(vec![
		r#"{"id_token":"id-new","access_token":"access-new","refresh_token":"refresh-new"}"#,
	]);
	let usage_endpoint = super::start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
	]);
	let reset_credits_endpoint = super::start_codex_reset_credits_fixture_server(1);

	fs::write(
		&accounts_path,
		r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-old","access_token":"access-old-pool","refresh_token":"refresh-old-pool","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("accounts fixture should write");
	fs::create_dir_all(codex_auth_path.parent().expect("auth path should have parent"))
		.expect("auth parent should create");
	fs::write(
		&codex_auth_path,
		r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-current","access_token":"access-current","refresh_token":"refresh-current","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("Codex auth fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account_and_codex_auth_path(
		&accounts_path,
		usage_endpoint,
		reset_credits_endpoint,
		refresh_endpoint,
		None,
		codex_auth_path.clone(),
	)
	.expect("account pool should initialize");
	let account = pool.refresh_account(Some("acct_sync")).expect("account should refresh");

	assert_eq!(account.access_token(), "access-new");

	let accounts_input = fs::read_to_string(&accounts_path).expect("accounts should read");

	assert!(accounts_input.contains(r#""access_token":"access-new""#));
	assert!(accounts_input.contains(r#""refresh_token":"refresh-new""#));

	let codex_auth = fs::read_to_string(&codex_auth_path).expect("Codex auth should read");
	let codex_auth_json =
		serde_json::from_str::<Value>(&codex_auth).expect("Codex auth should parse");

	assert_eq!(codex_auth_json["tokens"]["account_id"], "acct_sync");
	assert_eq!(codex_auth_json["tokens"]["id_token"], "id-new");
	assert_eq!(codex_auth_json["tokens"]["access_token"], "access-new");
	assert_eq!(codex_auth_json["tokens"]["refresh_token"], "refresh-new");
}

#[test]
fn token_refresh_leaves_nonmatching_codex_auth_json_unchanged() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let codex_auth_path = temp_dir.path().join(".codex/auth.json");
	let refresh_endpoint = super::start_codex_refresh_fixture_server(vec![
		r#"{"id_token":"id-new","access_token":"access-new","refresh_token":"refresh-new"}"#,
	]);
	let usage_endpoint = super::start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
	]);
	let reset_credits_endpoint = super::start_codex_reset_credits_fixture_server(1);

	fs::write(
		&accounts_path,
		r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-old","access_token":"access-old-pool","refresh_token":"refresh-old-pool","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("accounts fixture should write");
	fs::create_dir_all(codex_auth_path.parent().expect("auth path should have parent"))
		.expect("auth parent should create");
	fs::write(
		&codex_auth_path,
		r#"{"email":"other@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-other","access_token":"access-other","refresh_token":"refresh-other","account_id":"acct_other"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("Codex auth fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account_and_codex_auth_path(
		&accounts_path,
		usage_endpoint,
		reset_credits_endpoint,
		refresh_endpoint,
		None,
		codex_auth_path.clone(),
	)
	.expect("account pool should initialize");
	let account = pool.refresh_account(Some("acct_sync")).expect("account should refresh");

	assert_eq!(account.access_token(), "access-new");

	let codex_auth = fs::read_to_string(&codex_auth_path).expect("Codex auth should read");
	let codex_auth_json =
		serde_json::from_str::<Value>(&codex_auth).expect("Codex auth should parse");

	assert_eq!(codex_auth_json["tokens"]["account_id"], "acct_other");
	assert_eq!(codex_auth_json["tokens"]["id_token"], "id-other");
	assert_eq!(codex_auth_json["tokens"]["access_token"], "access-other");
	assert_eq!(codex_auth_json["tokens"]["refresh_token"], "refresh-other");
}
