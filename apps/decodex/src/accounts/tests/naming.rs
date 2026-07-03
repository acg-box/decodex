use std::fs;

use tempfile::TempDir;

use crate::accounts::{store::AccountStore, tests};

#[test]
fn reroll_name_persists_global_account_name_offset() {
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

	let initial = store.list().expect("account list should load");
	let updated = store.reroll_name("copy@example.com", None).expect("account name should reroll");
	let reloaded = store.list().expect("account list should reload");

	assert_eq!(initial.accounts[0].random_name_offset, 0);
	assert_eq!(updated.accounts[0].random_name_offset, 1);
	assert_ne!(initial.accounts[0].random_name, updated.accounts[0].random_name);
	assert_eq!(reloaded.accounts[0].random_name, updated.accounts[0].random_name);
	assert_eq!(reloaded.accounts[0].random_name_key, updated.accounts[0].random_name_key);
	assert!(
		fs::read_to_string(&store.global_config_path)
			.expect("global config should read")
			.contains("[codex.account_names.offsets]")
	);
}

#[test]
fn list_response_disambiguates_colliding_random_names() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[
			tests::account_record(
				"first@example.com",
				"acct_000023",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret-1",
			),
			tests::account_record(
				"second@example.com",
				"acct_000030",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret-2",
			),
		])
		.expect("records should save");

	let response = store.list().expect("account list should load");

	assert_eq!(response.accounts[0].random_name, "Reese");
	assert_eq!(response.accounts[1].random_name, "Remy");
	assert_ne!(response.accounts[0].random_name, response.accounts[1].random_name);
}
