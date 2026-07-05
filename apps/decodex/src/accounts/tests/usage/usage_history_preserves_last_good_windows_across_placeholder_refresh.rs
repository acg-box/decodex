use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	accounts::{store::AccountStore, tests},
	state::CodexAccountActivitySummary,
};

#[test]
fn usage_history_preserves_last_good_windows_across_placeholder_refresh() {
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

	let good_summary = CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(now),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(now + 18_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(now + 604_800),
		..CodexAccountActivitySummary::default()
	};
	let mut response = store.list().expect("account list should load");

	response.apply_usage_summaries(&[good_summary]);
	response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

	let degraded_summary = CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(now + 60),
		profile_lifetime_tokens: Some(47_200_000_000),
		..CodexAccountActivitySummary::default()
	};
	let mut degraded_response = store.list().expect("account list should reload");

	degraded_response.apply_usage_summaries(&[degraded_summary]);
	degraded_response
		.refresh_usage_records(&store.accounts_path)
		.expect("usage history should restore usable windows");

	let account = &degraded_response.accounts[0];

	assert_eq!(account.primary_window_seconds, Some(18_000));
	assert_eq!(account.primary_remaining_percent, Some(72));
	assert_eq!(account.primary_resets_at_unix_epoch, Some(now + 18_000));
	assert_eq!(account.secondary_window_seconds, Some(604_800));
	assert_eq!(account.secondary_remaining_percent, Some(91));
	assert_eq!(account.secondary_resets_at_unix_epoch, Some(now + 604_800));
	assert_eq!(account.seven_day_used_percent, Some(9));
	assert_eq!(account.profile_lifetime_tokens, Some(47_200_000_000));
}
