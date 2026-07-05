use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	agent::codex_accounts::usage::{
		AccountProfileSnapshot, AccountUsageSnapshot, ResetCreditSummary, ResetCreditsSnapshot,
	},
	state::CodexAccountProfileDailyUsageSummary,
};

pub(crate) fn usage_snapshot_from_payload(
	payload: &Value,
	checked_at_unix_epoch: i64,
) -> AccountUsageSnapshot {
	let rate_limit = payload.get("rate_limit").filter(|value| !value.is_null());

	AccountUsageSnapshot {
		plan_type: payload.get("plan_type").and_then(json_scalar_to_string),
		primary: rate_limit
			.and_then(|details| usage_window_from_value(details.get("primary_window"))),
		secondary: rate_limit
			.and_then(|details| usage_window_from_value(details.get("secondary_window"))),
		credits: payload.get("credits").and_then(credits_from_value),
		rate_limit_reached_type: rate_limit_reached_type_from_payload(payload),
		checked_at_unix_epoch,
	}
}

pub(crate) fn profile_snapshot_from_payload(
	payload: &Value,
	checked_at_unix_epoch: i64,
) -> Option<AccountProfileSnapshot> {
	let profile = payload.get("profile").filter(|value| !value.is_null());
	let stats = payload.get("stats").filter(|value| !value.is_null());
	let daily_usage = stats
		.and_then(|value| value.get("daily_usage_buckets"))
		.and_then(Value::as_array)
		.map(|items| items.iter().filter_map(profile_daily_usage_from_value).collect::<Vec<_>>())
		.unwrap_or_default();
	let peak_daily_tokens = stats
		.and_then(|value| nonnegative_number_as_i64(value.get("peak_daily_tokens")))
		.or_else(|| daily_usage.iter().map(|record| record.tokens).max());
	let snapshot = AccountProfileSnapshot {
		display_name: profile.and_then(|value| nonblank_json_string(value.get("display_name"))),
		username: profile.and_then(|value| nonblank_json_string(value.get("username"))),
		lifetime_tokens: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("lifetime_tokens"))),
		peak_daily_tokens,
		longest_task_seconds: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("longest_running_turn_sec"))),
		current_streak_days: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("current_streak_days"))),
		longest_streak_days: stats
			.and_then(|value| nonnegative_number_as_i64(value.get("longest_streak_days"))),
		daily_usage,
		checked_at_unix_epoch,
	};

	(!snapshot.is_empty()).then_some(snapshot)
}

pub(crate) fn reset_credits_snapshot_from_payload(
	payload: &Value,
	checked_at_unix_epoch: i64,
) -> ResetCreditsSnapshot {
	let credits = payload
		.get("credits")
		.and_then(Value::as_array)
		.map(|items| items.iter().filter_map(reset_credit_from_value).collect::<Vec<_>>())
		.unwrap_or_default();

	ResetCreditsSnapshot {
		available_count: nonnegative_number_as_i64(payload.get("available_count")),
		total_earned_count: nonnegative_number_as_i64(payload.get("total_earned_count")),
		credits,
		checked_at_unix_epoch,
	}
}

pub(crate) fn number_as_i64(value: &Value) -> Option<i64> {
	value
		.as_i64()
		.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
		.or_else(|| value.as_f64().map(|number| number.round() as i64))
}

pub(crate) fn json_scalar_to_string(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

pub(crate) fn nonblank_string(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn profile_daily_usage_from_value(value: &Value) -> Option<CodexAccountProfileDailyUsageSummary> {
	let date = nonblank_json_string(value.get("start_date"))?;
	let tokens = nonnegative_number_as_i64(value.get("tokens"))?;

	Some(CodexAccountProfileDailyUsageSummary { date, tokens })
}

fn usage_window_from_value(
	value: Option<&Value>,
) -> Option<crate::agent::codex_accounts::usage::UsageWindow> {
	let value = value.filter(|value| !value.is_null())?;
	let used_percent = number_as_i64(value.get("used_percent")?)?;
	let window_seconds = value.get("limit_window_seconds").and_then(number_as_i64);

	if window_seconds.is_some_and(|seconds| seconds <= 0) {
		return None;
	}

	let remaining_percent = 100_i64.saturating_sub(used_percent).clamp(0, 100);

	Some(crate::agent::codex_accounts::usage::UsageWindow {
		window_seconds,
		remaining_percent,
		resets_at_unix_epoch: value.get("reset_at").and_then(number_as_i64),
	})
}

fn credits_from_value(
	value: &Value,
) -> Option<crate::agent::codex_accounts::usage::CreditsSnapshot> {
	if value.is_null() {
		return None;
	}

	Some(crate::agent::codex_accounts::usage::CreditsSnapshot {
		has_credits: value.get("has_credits").and_then(Value::as_bool).unwrap_or(true),
		unlimited: value.get("unlimited").and_then(Value::as_bool).unwrap_or(false),
		balance: value.get("balance").and_then(json_scalar_to_string),
	})
}

fn rate_limit_reached_type_from_payload(payload: &Value) -> Option<String> {
	let reached = payload.get("rate_limit_reached_type").filter(|value| !value.is_null())?;

	if let Some(kind) = reached.get("kind").and_then(json_scalar_to_string) {
		return Some(kind);
	}

	json_scalar_to_string(reached)
}

fn nonnegative_number_as_i64(value: Option<&Value>) -> Option<i64> {
	value.and_then(number_as_i64).map(|number| number.max(0))
}

fn nonblank_json_string(value: Option<&Value>) -> Option<String> {
	value.and_then(json_scalar_to_string).and_then(|value| nonblank_string(Some(&value)))
}

fn reset_credit_from_value(value: &Value) -> Option<ResetCreditSummary> {
	let status = nonblank_json_string(value.get("status"));
	let granted_at_unix_epoch =
		value.get("granted_at").and_then(json_scalar_to_string).and_then(rfc3339_unix_epoch);
	let expires_at_unix_epoch =
		value.get("expires_at").and_then(json_scalar_to_string).and_then(rfc3339_unix_epoch);

	(granted_at_unix_epoch.is_some() || expires_at_unix_epoch.is_some())
		.then_some(ResetCreditSummary { granted_at_unix_epoch, expires_at_unix_epoch, status })
}

fn rfc3339_unix_epoch(value: String) -> Option<i64> {
	OffsetDateTime::parse(value.trim(), &Rfc3339).ok().map(|timestamp| timestamp.unix_timestamp())
}
