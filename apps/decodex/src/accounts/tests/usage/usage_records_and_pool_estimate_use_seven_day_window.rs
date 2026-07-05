use std::fs;

use tempfile::TempDir;

use crate::accounts::{self, store::AccountStore, tests};

#[test]
fn usage_records_and_pool_estimate_use_seven_day_window() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
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

	let summaries = [
		tests::usage_summary("copy@example.com", "...123456", "pro", 40),
		tests::usage_summary("other@example.com", "...654321", "plus", 70),
	];
	let mut response = store.list().expect("account list should load");

	response.apply_usage_summaries(&summaries);
	response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

	let estimate = response.usage_estimate.as_ref().expect("usage estimate should exist");
	let history_path = accounts::usage_history_path(&store.accounts_path)
		.expect("usage history path should resolve");
	let history = fs::read_to_string(history_path).expect("usage history should read");
	let record_date =
		accounts::usage_record_date(1_800_000_000).expect("usage record date should format");

	assert_eq!(estimate.window_days, 7);
	assert_eq!(estimate.account_count, 2);
	assert_eq!(estimate.account_estimate_count, 2);
	assert_eq!(estimate.total_capacity_percent, 2_100);
	assert_eq!(estimate.total_used_percent, 1_230);

	tests::assert_close(Some(estimate.total_used_of_capacity_percent), 58.571);
	tests::assert_close(Some(estimate.average_daily_used_percent), 1_230.0 / 7.0);
	tests::assert_close(Some(estimate.average_daily_pool_percent), 58.571 / 7.0);

	assert_eq!(response.accounts[0].usage_records.len(), 1);
	assert_eq!(response.accounts[0].usage_records[0].date, record_date);
	assert_eq!(response.accounts[0].usage_records[0].used_percent, 60);
	assert_eq!(response.accounts[0].usage_records[0].capacity_multiplier, 20);
	assert_eq!(response.accounts[1].usage_records[0].capacity_multiplier, 1);
	assert_eq!(history.lines().count(), 2);
	assert!(history.contains(r#""used_percent":60"#));
	assert!(history.contains(r#""capacity_multiplier":20"#));
	assert!(history.contains(r#""used_percent":30"#));
	assert!(history.contains(r#""capacity_multiplier":1"#));
}
