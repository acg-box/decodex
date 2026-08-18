use std::path::PathBuf;

use serde::Serialize;

use crate::{
	accounts::usage_history::{AccountUsageDailySummary, AccountUsageEstimateSummary},
	state::{CodexAccountProfileDailyUsageSummary, CodexAccountResetCreditSummary},
};

pub(crate) struct AccountImportRequest {
	pub(crate) auth_json_path: PathBuf,
	pub(crate) json: bool,
}

pub(crate) struct AccountUseRequest {
	pub(crate) selector: String,
	pub(crate) auth_json_path: Option<PathBuf>,
	pub(crate) json: bool,
}

#[derive(Serialize)]
pub(crate) struct AccountListResponse {
	pub(crate) accounts_path: String,
	pub(crate) global_config_path: String,
	pub(crate) codex_auth_path: String,
	pub(crate) codex_auth: Option<AccountIdentitySummary>,
	pub(crate) control: AccountControlSummary,
	pub(crate) accounts: Vec<AccountSummary>,
	pub(crate) usage_estimate: Option<AccountUsageEstimateSummary>,
	pub(crate) usage_probe_error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AccountUseResponse {
	pub(crate) codex_auth_path: String,
	pub(crate) account: AccountIdentitySummary,
}

#[derive(Clone, Serialize)]
pub(crate) struct AccountIdentitySummary {
	pub(crate) account_fingerprint: String,
	pub(crate) email: Option<String>,
	pub(crate) selector: String,
}

#[derive(Serialize)]
pub(crate) struct AccountControlSummary {
	pub(crate) mode: String,
	pub(crate) account_selector: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AccountSummary {
	pub(crate) account_fingerprint: String,
	pub(crate) email: Option<String>,
	pub(crate) selector: String,
	pub(crate) random_name: String,
	pub(crate) random_name_key: String,
	pub(crate) random_name_offset: i64,
	pub(crate) status: String,
	pub(crate) selected: bool,
	pub(crate) codex_active: bool,
	pub(crate) disabled: bool,
	pub(crate) refresh_token_present: bool,
	pub(crate) access_token_expires_at_unix_epoch: Option<i64>,
	pub(crate) last_selected_at_unix_epoch: Option<i64>,
	pub(crate) cooldown_until_unix_epoch: Option<i64>,
	pub(crate) note: Option<String>,
	pub(crate) plan_type: Option<String>,
	pub(crate) capacity_multiplier: i64,
	pub(crate) recovery_action: Option<String>,
	pub(crate) refresh_status: Option<String>,
	pub(crate) checked_at_unix_epoch: Option<i64>,
	pub(crate) primary_window_seconds: Option<i64>,
	pub(crate) primary_remaining_percent: Option<i64>,
	pub(crate) primary_resets_at_unix_epoch: Option<i64>,
	pub(crate) secondary_window_seconds: Option<i64>,
	pub(crate) secondary_remaining_percent: Option<i64>,
	pub(crate) secondary_resets_at_unix_epoch: Option<i64>,
	pub(crate) credits_has_credits: Option<bool>,
	pub(crate) credits_unlimited: Option<bool>,
	pub(crate) credits_balance: Option<String>,
	pub(crate) reset_credits_available_count: Option<i64>,
	pub(crate) reset_credits_total_earned_count: Option<i64>,
	pub(crate) reset_credits_checked_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub(crate) reset_credits: Vec<CodexAccountResetCreditSummary>,
	pub(crate) rate_limit_reached_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_display_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_username: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_checked_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_lifetime_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_peak_daily_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_task_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_current_streak_days: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_streak_days: Option<i64>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub(crate) profile_daily_usage: Vec<CodexAccountProfileDailyUsageSummary>,
	pub(crate) seven_day_used_percent: Option<i64>,
	pub(crate) seven_day_daily_average_percent: Option<f64>,
	pub(crate) usage_records: Vec<AccountUsageDailySummary>,
}
