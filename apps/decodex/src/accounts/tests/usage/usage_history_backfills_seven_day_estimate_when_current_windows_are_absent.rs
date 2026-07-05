use std::fs;

use tempfile::TempDir;

use crate::accounts::{self, store::AccountStore, tests};

#[test]
fn usage_history_backfills_seven_day_estimate_when_current_windows_are_absent() {
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

	let history_path = accounts::usage_history_path(&store.accounts_path)
		.expect("usage history path should resolve");

	fs::create_dir_all(history_path.parent().expect("history path should have parent"))
		.expect("history dir should create");
	fs::write(
		&history_path,
		r#"{"date":"2026-05-27","account_fingerprint":"...123456","email":"copy@example.com","used_percent":22,"window_seconds":604800,"checked_at_unix_epoch":1800000000,"resets_at_unix_epoch":1800604800}
{"date":"2026-05-28","account_fingerprint":"...123456","email":"copy@example.com","used_percent":63,"window_seconds":604800,"checked_at_unix_epoch":1800000100,"resets_at_unix_epoch":1800604900}
"#,
	)
	.expect("usage history should write");

	let mut response = store.list().expect("account list should load");

	response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

	let estimate = response.usage_estimate.as_ref().expect("usage estimate should exist");

	assert_eq!(response.accounts[0].primary_remaining_percent, None);
	assert_eq!(response.accounts[0].seven_day_used_percent, Some(63));

	tests::assert_close(response.accounts[0].seven_day_daily_average_percent, 63.0 / 7.0);

	assert_eq!(response.accounts[0].usage_records.len(), 2);
	assert_eq!(estimate.account_estimate_count, 1);
	assert_eq!(estimate.total_used_percent, 63);
}
