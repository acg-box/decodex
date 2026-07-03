mod naming;
mod store;
mod usage;

use crate::{
	accounts::{auth_json::CodexTokenData, record::AccountPoolRecord},
	state::CodexAccountActivitySummary,
};

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
