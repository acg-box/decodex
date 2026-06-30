use std::{
	error::Error,
	fmt::{self, Display, Formatter},
};

use serde_json::Value;

use super::selection::account_summary_is_limited;
use crate::state::{CodexAccountActivitySummary, CodexAccountProfileDailyUsageSummary};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AccountUsageSnapshot {
	pub(crate) plan_type: Option<String>,
	pub(crate) primary: Option<UsageWindow>,
	pub(crate) secondary: Option<UsageWindow>,
	pub(crate) credits: Option<CreditsSnapshot>,
	pub(crate) rate_limit_reached_type: Option<String>,
	pub(crate) checked_at_unix_epoch: i64,
}
impl AccountUsageSnapshot {
	pub(super) fn is_limited(&self) -> bool {
		self.rate_limit_reached_type.is_some()
			|| self.primary.as_ref().is_some_and(UsageWindow::is_depleted)
			|| self.secondary.as_ref().is_some_and(UsageWindow::is_depleted)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AccountProfileSnapshot {
	pub(crate) display_name: Option<String>,
	pub(crate) username: Option<String>,
	pub(crate) lifetime_tokens: Option<i64>,
	pub(crate) peak_daily_tokens: Option<i64>,
	pub(crate) longest_task_seconds: Option<i64>,
	pub(crate) current_streak_days: Option<i64>,
	pub(crate) longest_streak_days: Option<i64>,
	pub(crate) daily_usage: Vec<CodexAccountProfileDailyUsageSummary>,
	pub(crate) checked_at_unix_epoch: i64,
}
impl AccountProfileSnapshot {
	fn is_empty(&self) -> bool {
		self.display_name.is_none()
			&& self.username.is_none()
			&& self.lifetime_tokens.is_none()
			&& self.peak_daily_tokens.is_none()
			&& self.longest_task_seconds.is_none()
			&& self.current_streak_days.is_none()
			&& self.longest_streak_days.is_none()
			&& self.daily_usage.is_empty()
	}

	pub(super) fn apply_to_summary(self, summary: &mut CodexAccountActivitySummary) {
		summary.profile_display_name = self.display_name;
		summary.profile_username = self.username;
		summary.profile_checked_at_unix_epoch = Some(self.checked_at_unix_epoch);
		summary.profile_lifetime_tokens = self.lifetime_tokens;
		summary.profile_peak_daily_tokens = self.peak_daily_tokens;
		summary.profile_longest_task_seconds = self.longest_task_seconds;
		summary.profile_current_streak_days = self.current_streak_days;
		summary.profile_longest_streak_days = self.longest_streak_days;
		summary.profile_daily_usage = self.daily_usage;
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UsageWindow {
	pub(crate) window_seconds: Option<i64>,
	pub(crate) remaining_percent: i64,
	pub(crate) resets_at_unix_epoch: Option<i64>,
}
impl UsageWindow {
	const fn is_depleted(&self) -> bool {
		self.remaining_percent <= 0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CreditsSnapshot {
	pub(crate) has_credits: bool,
	pub(crate) unlimited: bool,
	pub(crate) balance: Option<String>,
}

#[derive(Debug)]
pub(super) struct UsageProbeError {
	pub(super) unauthorized: bool,
	message: String,
}
impl UsageProbeError {
	pub(super) fn unauthorized() -> Self {
		Self { unauthorized: true, message: String::from("usage endpoint returned 401") }
	}

	pub(super) fn other(message: impl Into<String>) -> Self {
		Self { unauthorized: false, message: message.into() }
	}
}

impl Display for UsageProbeError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.message)
	}
}

impl Error for UsageProbeError {}

pub(super) fn usage_snapshot_from_payload(
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

pub(super) fn profile_snapshot_from_payload(
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

fn profile_daily_usage_from_value(value: &Value) -> Option<CodexAccountProfileDailyUsageSummary> {
	let date = nonblank_json_string(value.get("start_date"))?;
	let tokens = nonnegative_number_as_i64(value.get("tokens"))?;

	Some(CodexAccountProfileDailyUsageSummary { date, tokens })
}

fn usage_window_from_value(value: Option<&Value>) -> Option<UsageWindow> {
	let value = value.filter(|value| !value.is_null())?;
	let used_percent = number_as_i64(value.get("used_percent")?)?;
	let window_seconds = value.get("limit_window_seconds").and_then(number_as_i64);

	if window_seconds.is_some_and(|seconds| seconds <= 0) {
		return None;
	}

	let remaining_percent = 100_i64.saturating_sub(used_percent).clamp(0, 100);

	Some(UsageWindow {
		window_seconds,
		remaining_percent,
		resets_at_unix_epoch: value.get("reset_at").and_then(number_as_i64),
	})
}

pub(super) fn preserve_cached_usage_windows(
	summaries: &mut [CodexAccountActivitySummary],
	cached_summaries: &[CodexAccountActivitySummary],
	now_unix_epoch: i64,
) {
	for summary in summaries {
		if account_summary_is_limited(summary) {
			continue;
		}

		let Some(cached) =
			cached_summaries.iter().find(|cached| account_summaries_match(summary, cached))
		else {
			continue;
		};

		preserve_primary_usage_window(summary, cached, now_unix_epoch);
		preserve_secondary_usage_window(summary, cached, now_unix_epoch);
	}
}

fn preserve_primary_usage_window(
	summary: &mut CodexAccountActivitySummary,
	cached: &CodexAccountActivitySummary,
	now_unix_epoch: i64,
) {
	if has_usage_window(summary.primary_window_seconds, summary.primary_remaining_percent)
		|| !has_current_usage_window(
			cached.primary_window_seconds,
			cached.primary_remaining_percent,
			cached.primary_resets_at_unix_epoch,
			now_unix_epoch,
		) {
		return;
	}

	summary.primary_window_seconds = cached.primary_window_seconds;
	summary.primary_remaining_percent = cached.primary_remaining_percent;
	summary.primary_resets_at_unix_epoch = cached.primary_resets_at_unix_epoch;
}

fn preserve_secondary_usage_window(
	summary: &mut CodexAccountActivitySummary,
	cached: &CodexAccountActivitySummary,
	now_unix_epoch: i64,
) {
	if has_usage_window(summary.secondary_window_seconds, summary.secondary_remaining_percent)
		|| !has_current_usage_window(
			cached.secondary_window_seconds,
			cached.secondary_remaining_percent,
			cached.secondary_resets_at_unix_epoch,
			now_unix_epoch,
		) {
		return;
	}

	summary.secondary_window_seconds = cached.secondary_window_seconds;
	summary.secondary_remaining_percent = cached.secondary_remaining_percent;
	summary.secondary_resets_at_unix_epoch = cached.secondary_resets_at_unix_epoch;
}

const fn has_usage_window(window_seconds: Option<i64>, remaining_percent: Option<i64>) -> bool {
	matches!(window_seconds, Some(seconds) if seconds > 0) && remaining_percent.is_some()
}

fn has_current_usage_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> bool {
	has_usage_window(window_seconds, remaining_percent)
		&& resets_at_unix_epoch.is_some_and(|reset| reset > now_unix_epoch)
}

fn account_summaries_match(
	left: &CodexAccountActivitySummary,
	right: &CodexAccountActivitySummary,
) -> bool {
	left.account_fingerprint == right.account_fingerprint
		|| left
			.email
			.as_deref()
			.zip(right.email.as_deref())
			.is_some_and(|(left, right)| left == right)
}

fn credits_from_value(value: &Value) -> Option<CreditsSnapshot> {
	if value.is_null() {
		return None;
	}

	Some(CreditsSnapshot {
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

pub(super) fn number_as_i64(value: &Value) -> Option<i64> {
	value
		.as_i64()
		.or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
		.or_else(|| value.as_f64().map(|number| number.round() as i64))
}

fn nonnegative_number_as_i64(value: Option<&Value>) -> Option<i64> {
	value.and_then(number_as_i64).map(|number| number.max(0))
}

pub(super) fn json_scalar_to_string(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

fn nonblank_json_string(value: Option<&Value>) -> Option<String> {
	value.and_then(json_scalar_to_string).and_then(|value| nonblank_string(Some(&value)))
}

pub(super) fn nonblank_string(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}
