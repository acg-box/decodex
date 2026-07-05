use tempfile::TempDir;

use crate::{
	accounts::{store::AccountStore, tests},
	state::{CodexAccountActivitySummary, CodexAccountProfileDailyUsageSummary},
};

#[test]
fn list_response_merges_usage_snapshot() {
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
		plan_type: Some(String::from("pro")),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_800_000_000),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(1_800_018_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(1_800_604_800),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("9.99")),
		rate_limit_reached_type: None,
		profile_lifetime_tokens: Some(47_200_000_000),
		profile_peak_daily_tokens: Some(1_500_000_000),
		profile_longest_task_seconds: Some(10_080),
		profile_current_streak_days: Some(12),
		profile_longest_streak_days: Some(68),
		profile_daily_usage: vec![CodexAccountProfileDailyUsageSummary {
			date: String::from("2026-05-31"),
			tokens: 123_456,
		}],
		..CodexAccountActivitySummary::default()
	}]);

	assert_eq!(response.accounts[0].plan_type.as_deref(), Some("pro"));
	assert_eq!(response.accounts[0].primary_window_seconds, Some(18_000));
	assert_eq!(response.accounts[0].primary_remaining_percent, Some(72));
	assert_eq!(response.accounts[0].secondary_window_seconds, Some(604_800));
	assert_eq!(response.accounts[0].secondary_remaining_percent, Some(91));
	assert_eq!(response.accounts[0].credits_balance.as_deref(), Some("9.99"));
	assert_eq!(response.accounts[0].profile_lifetime_tokens, Some(47_200_000_000));
	assert_eq!(response.accounts[0].profile_peak_daily_tokens, Some(1_500_000_000));
	assert_eq!(response.accounts[0].profile_longest_task_seconds, Some(10_080));
	assert_eq!(response.accounts[0].profile_current_streak_days, Some(12));
	assert_eq!(response.accounts[0].profile_longest_streak_days, Some(68));
	assert_eq!(response.accounts[0].profile_daily_usage[0].date, "2026-05-31");
	assert_eq!(response.accounts[0].seven_day_used_percent, Some(9));
	assert_eq!(response.accounts[0].capacity_multiplier, 20);
	assert_eq!(response.accounts[0].recovery_action, None);

	tests::assert_close(response.accounts[0].seven_day_daily_average_percent, 9.0 / 7.0);
}
