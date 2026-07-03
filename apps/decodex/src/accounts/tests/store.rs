use std::fs;

use tempfile::TempDir;

use crate::accounts::{
	auth_json::{AuthDotJson, CodexTokenData},
	record::AccountPoolRecord,
	store::AccountStore,
	tests,
};

#[test]
fn imports_auth_json_without_printing_tokens() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let auth_path = temp_dir.path().join("auth.json");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	fs::write(
		&auth_path,
		r#"{
			"email": "copy@example.com",
			"tokens": {
				"access_token": "header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh_token": "refresh-secret",
				"account_id": "acct_123456"
			}
		}"#,
	)
	.expect("auth json should write");

	let response = store.import_auth_json(&auth_path).expect("auth should import");
	let output = serde_json::to_string(&response).expect("response should serialize");

	assert_eq!(response.accounts.len(), 1);
	assert!(output.contains("copy@example.com"));
	assert!(output.contains("...123456"));
	assert!(!output.contains("refresh-secret"));
}

#[test]
fn logout_removes_matching_account() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[AccountPoolRecord {
			email: Some(String::from("copy@example.com")),
			disabled: false,
			cooldown_until_unix_epoch: None,
			cooldown_until: None,
			last_selected_at_unix_epoch: None,
			auth_failed_at_unix_epoch: None,
			auth_failure: None,
			auth_mode: None,
			openai_api_key: None,
			tokens: Some(CodexTokenData {
				email: None,
				id_token: None,
				access_token: String::from("token"),
				refresh_token: String::from("refresh"),
				account_id: Some(String::from("acct_123456")),
			}),
			last_refresh: None,
		}])
		.expect("records should save");

	let response = store.logout("copy@example.com").expect("account should logout");

	assert!(response.accounts.is_empty());
	assert_eq!(fs::read_to_string(&store.accounts_path).expect("accounts should read"), "");
}

#[test]
fn use_for_codex_overwrites_auth_json_from_pool() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let codex_auth_path = temp_dir.path().join(".codex/auth.json");
	let store = AccountStore::new_with_codex_auth_path(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
		codex_auth_path.clone(),
	);

	store
		.save_records(&[tests::account_record(
			"copy@example.com",
			"acct_123456",
			"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
			"refresh-secret",
		)])
		.expect("records should save");

	let response =
		store.use_for_codex("copy@example.com", None).expect("account should become Codex auth");
	let auth_input = fs::read_to_string(&codex_auth_path).expect("Codex auth should be written");
	let auth = serde_json::from_str::<AuthDotJson>(&auth_input).expect("Codex auth should parse");
	let tokens = auth.tokens.expect("Codex auth should include tokens");

	assert_eq!(response.account.email.as_deref(), Some("copy@example.com"));
	assert_eq!(auth.email.as_deref(), Some("copy@example.com"));
	assert_eq!(tokens.account_id.as_deref(), Some("acct_123456"));
}

#[test]
fn use_for_codex_rejects_auth_failed_account() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let codex_auth_path = temp_dir.path().join(".codex/auth.json");
	let store = AccountStore::new_with_codex_auth_path(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
		codex_auth_path,
	);
	let mut record = tests::account_record(
		"copy@example.com",
		"acct_123456",
		"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
		"refresh-secret",
	);

	record.auth_failed_at_unix_epoch = Some(1_800_000_000);
	record.auth_failure = Some(String::from(
		"Codex account `copy@example.com` token refresh failed with HTTP 401 Unauthorized.",
	));

	store.save_records(&[record]).expect("records should save");

	let error = match store.use_for_codex("copy@example.com", None) {
		Ok(_) => panic!("auth failed account should reject"),
		Err(error) => error,
	};

	assert!(error.to_string().contains("auth_failed"));
}

#[test]
fn list_marks_codex_active_account() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let codex_auth_path = temp_dir.path().join("auth.json");
	let store = AccountStore::new_with_codex_auth_path(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
		codex_auth_path.clone(),
	);

	store
		.save_records(&[
			tests::account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			),
			tests::account_record(
				"other@example.com",
				"acct_654321",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret-2",
			),
		])
		.expect("records should save");
	store.use_for_codex("other@example.com", None).expect("account should become Codex auth");

	let response = store.list().expect("account list should load");

	assert_eq!(
		response.codex_auth.as_ref().and_then(|auth| auth.email.as_deref()),
		Some("other@example.com")
	);
	assert!(!response.accounts[0].codex_active);
	assert!(response.accounts[1].codex_active);
}
