use std::fs;

use tempfile::TempDir;

use crate::{
	accounts::{AccountPoolRecord, AccountStore, AuthDotJson, CodexTokenData},
	state::{CodexAccountActivitySummary, CodexAccountProfileDailyUsageSummary},
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
		.save_records(&[account_record(
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
	let mut record = account_record(
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
			account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			),
			account_record(
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

#[test]
fn reroll_name_persists_global_account_name_offset() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[account_record(
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
			account_record(
				"first@example.com",
				"acct_000023",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret-1",
			),
			account_record(
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

#[test]
fn list_response_merges_usage_snapshot() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[account_record(
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

	assert_close(response.accounts[0].seven_day_daily_average_percent, 9.0 / 7.0);
}

#[test]
fn usage_summary_marks_refresh_401_as_login_recovery() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[account_record(
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
		status: String::from("unusable"),
		refresh_status: String::from("failed"),
		note: Some(String::from(
			"usage probe failed: Codex account `copy@example.com` token refresh failed with HTTP 401 Unauthorized.",
		)),
		..CodexAccountActivitySummary::default()
	}]);

	assert_eq!(response.accounts[0].recovery_action.as_deref(), Some("login"));
}

#[test]
fn usage_records_and_pool_estimate_use_seven_day_window() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[
			account_record(
				"copy@example.com",
				"acct_123456",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret",
			),
			account_record(
				"other@example.com",
				"acct_654321",
				"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
				"refresh-secret-2",
			),
		])
		.expect("records should save");

	let summaries = [
		usage_summary("copy@example.com", "...123456", "pro", 40),
		usage_summary("other@example.com", "...654321", "plus", 70),
	];
	let mut response = store.list().expect("account list should load");

	response.apply_usage_summaries(&summaries);
	response.refresh_usage_records(&store.accounts_path).expect("usage history should refresh");

	let estimate = response.usage_estimate.as_ref().expect("usage estimate should exist");
	let history_path =
		super::usage_history_path(&store.accounts_path).expect("usage history path should resolve");
	let history = fs::read_to_string(history_path).expect("usage history should read");
	let record_date =
		super::usage_record_date(1_800_000_000).expect("usage record date should format");

	assert_eq!(estimate.window_days, 7);
	assert_eq!(estimate.account_count, 2);
	assert_eq!(estimate.account_estimate_count, 2);
	assert_eq!(estimate.total_capacity_percent, 2_100);
	assert_eq!(estimate.total_used_percent, 1_230);

	assert_close(Some(estimate.total_used_of_capacity_percent), 58.571);
	assert_close(Some(estimate.average_daily_used_percent), 1_230.0 / 7.0);
	assert_close(Some(estimate.average_daily_pool_percent), 58.571 / 7.0);

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

#[test]
fn capacity_multiplier_counts_only_pro_above_plus_weight() {
	assert_eq!(super::account_capacity_multiplier(Some("pro")), 20);
	assert_eq!(super::account_capacity_multiplier(Some("plus")), 1);
	assert_eq!(super::account_capacity_multiplier(Some("team")), 1);
	assert_eq!(super::account_capacity_multiplier(None), 1);
}

#[test]
fn usage_history_backfills_seven_day_estimate_when_current_windows_are_absent() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);

	store
		.save_records(&[account_record(
			"copy@example.com",
			"acct_123456",
			"header.eyJleHAiOjQxMDI0NDQ4MDB9.sig",
			"refresh-secret",
		)])
		.expect("records should save");

	let history_path =
		super::usage_history_path(&store.accounts_path).expect("usage history path should resolve");

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

	assert_close(response.accounts[0].seven_day_daily_average_percent, 63.0 / 7.0);

	assert_eq!(response.accounts[0].usage_records.len(), 2);
	assert_eq!(estimate.account_estimate_count, 1);
	assert_eq!(estimate.total_used_percent, 63);
}

#[test]
fn usage_history_preserves_last_good_windows_across_placeholder_refresh() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let store = AccountStore::new(
		temp_dir.path().join("accounts.jsonl"),
		temp_dir.path().join("config.toml"),
	);
	let now = time::OffsetDateTime::now_utc().unix_timestamp();

	store
		.save_records(&[account_record(
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

fn account_record(
	email: &str,
	account_id: &str,
	access_token: &str,
	refresh_token: &str,
) -> AccountPoolRecord {
	AccountPoolRecord {
		email: Some(String::from(email)),
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
			access_token: String::from(access_token),
			refresh_token: String::from(refresh_token),
			account_id: Some(String::from(account_id)),
		}),
		last_refresh: None,
	}
}

fn usage_summary(
	email: &str,
	account_fingerprint: &str,
	plan_type: &str,
	secondary_remaining_percent: i64,
) -> CodexAccountActivitySummary {
	CodexAccountActivitySummary {
		account_fingerprint: String::from(account_fingerprint),
		email: Some(String::from(email)),
		plan_type: Some(String::from(plan_type)),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_800_000_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(secondary_remaining_percent),
		secondary_resets_at_unix_epoch: Some(1_800_604_800),
		..CodexAccountActivitySummary::default()
	}
}

fn assert_close(value: Option<f64>, expected: f64) {
	let value = value.expect("value should exist");

	assert!((value - expected).abs() < 0.001, "expected {value} to be within 0.001 of {expected}");
}
