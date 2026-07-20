use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	accounts::{store::AccountStore, tests},
	state::CodexAccountActivitySummary,
};

#[test]
fn usage_history_does_not_mirror_primary_into_secondary() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);
	let now = OffsetDateTime::now_utc().unix_timestamp();

	store
		.save_records(&[tests::account_record(
			"copy@example.com",
			"acct_123456",
			"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
			"refresh-secret",
		)])
		.expect("records should save");

	let summary = CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(now),
		primary_window_seconds: Some(604_800),
		primary_remaining_percent: Some(39),
		primary_resets_at_unix_epoch: Some(now + 604_800),
		..CodexAccountActivitySummary::default()
	};
	let mut response = store.list().expect("account list should load");

	response.apply_usage_summaries(&[summary]);
	response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

	let account = &response.accounts[0];

	assert_eq!(account.primary_window_seconds, Some(604_800));
	assert_eq!(account.primary_remaining_percent, Some(39));
	assert_eq!(account.primary_resets_at_unix_epoch, Some(now + 604_800));
	assert_eq!(account.secondary_window_seconds, None);
	assert_eq!(account.secondary_remaining_percent, None);
	assert_eq!(account.secondary_resets_at_unix_epoch, None);
	assert_eq!(account.seven_day_used_percent, Some(61));

	let degraded_summary = CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(now + 60),
		..CodexAccountActivitySummary::default()
	};
	let mut degraded_response = store.list().expect("account list should reload");

	degraded_response.apply_usage_summaries(&[degraded_summary]);
	degraded_response
		.refresh_usage_records(&store.accounts_path)
		.expect("usage history should restore the primary window");

	let restored_account = &degraded_response.accounts[0];

	assert_eq!(restored_account.primary_window_seconds, Some(604_800));
	assert_eq!(restored_account.primary_remaining_percent, Some(39));
	assert_eq!(restored_account.primary_resets_at_unix_epoch, Some(now + 604_800));
	assert_eq!(restored_account.secondary_window_seconds, None);
	assert_eq!(restored_account.secondary_remaining_percent, None);
	assert_eq!(restored_account.secondary_resets_at_unix_epoch, None);
	assert_eq!(restored_account.seven_day_used_percent, Some(61));
}
