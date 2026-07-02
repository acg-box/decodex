pub(crate) const DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER: i64 = 1;
pub(crate) const USAGE_ESTIMATE_WINDOW_DAYS: i64 = 7;
pub(crate) const USAGE_ESTIMATE_WINDOW_SECONDS: i64 = USAGE_ESTIMATE_WINDOW_DAYS * 24 * 60 * 60;

const PRO_ACCOUNT_CAPACITY_MULTIPLIER: i64 = 20;

pub(crate) fn accepts_secondary_usage_window(window_seconds: Option<i64>) -> bool {
	window_seconds.is_none_or(is_seven_day_usage_window)
}

pub(crate) const fn default_account_capacity_multiplier() -> i64 {
	DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER
}

pub(crate) fn account_capacity_multiplier(plan_type: Option<&str>) -> i64 {
	match plan_type.map(str::trim).filter(|value| !value.is_empty()) {
		Some(plan_type) if plan_type.eq_ignore_ascii_case("pro") => PRO_ACCOUNT_CAPACITY_MULTIPLIER,
		_ => DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER,
	}
}

pub(crate) fn normalized_account_capacity_multiplier(value: i64) -> i64 {
	value.max(DEFAULT_ACCOUNT_CAPACITY_MULTIPLIER)
}

pub(crate) fn is_seven_day_usage_window(window_seconds: i64) -> bool {
	window_seconds
		.checked_sub(USAGE_ESTIMATE_WINDOW_SECONDS)
		.is_some_and(|delta| delta.abs() <= 3_600)
}

pub(crate) fn has_usage_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
) -> bool {
	matches!(window_seconds, Some(seconds) if seconds > 0) && remaining_percent.is_some()
}

pub(crate) fn has_current_usage_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> bool {
	has_usage_window(window_seconds, remaining_percent)
		&& resets_at_unix_epoch.is_some_and(|reset| reset > now_unix_epoch)
}

pub(crate) fn used_percent_from_remaining(remaining_percent: i64) -> i64 {
	100_i64.saturating_sub(remaining_percent).clamp(0, 100)
}

pub(crate) fn remaining_percent_from_used(used_percent: i64) -> i64 {
	100_i64.saturating_sub(used_percent).clamp(0, 100)
}

pub(crate) fn percent_ratio(numerator: i64, denominator: i64) -> f64 {
	if denominator <= 0 {
		return 0.0;
	}

	(numerator as f64 / denominator as f64) * 100.0
}
