use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::state::{CodexAccountActivitySummary, CodexAccountProfileDailyUsageSummary};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AccountUsageSnapshot {
	pub(crate) plan_type: Option<String>,
	pub(crate) primary: Option<UsageWindow>,
	pub(crate) secondary: Option<UsageWindow>,
	pub(crate) credits: Option<CreditsSnapshot>,
	pub(crate) rate_limit_reached_type: Option<String>,
	pub(crate) checked_at_unix_epoch: i64,
}
impl AccountUsageSnapshot {
	pub(crate) fn is_limited(&self) -> bool {
		self.rate_limit_reached_type.is_some()
			|| self.primary.as_ref().is_some_and(UsageWindow::is_depleted)
			|| self.secondary.as_ref().is_some_and(UsageWindow::is_depleted)
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AccountProfileSnapshot {
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
	pub(crate) fn is_empty(&self) -> bool {
		self.display_name.is_none()
			&& self.username.is_none()
			&& self.lifetime_tokens.is_none()
			&& self.peak_daily_tokens.is_none()
			&& self.longest_task_seconds.is_none()
			&& self.current_streak_days.is_none()
			&& self.longest_streak_days.is_none()
			&& self.daily_usage.is_empty()
	}

	pub(crate) fn apply_to_summary(self, summary: &mut CodexAccountActivitySummary) {
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

#[derive(Debug)]
pub(crate) struct UsageProbeError {
	pub(crate) unauthorized: bool,
	message: String,
}
impl UsageProbeError {
	pub(crate) fn unauthorized(endpoint: &str) -> Self {
		Self {
			unauthorized: true,
			message: format!(
				"{endpoint} returned 401; credentials may be expired or the Authorization header may be missing or invalid"
			),
		}
	}

	pub(crate) fn other(message: impl Into<String>) -> Self {
		Self { unauthorized: false, message: message.into() }
	}
}

impl Display for UsageProbeError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.message)
	}
}

impl Error for UsageProbeError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UsageWindow {
	pub(crate) window_seconds: Option<i64>,
	pub(crate) remaining_percent: i64,
	pub(crate) resets_at_unix_epoch: Option<i64>,
}
impl UsageWindow {
	pub(crate) const fn is_depleted(&self) -> bool {
		self.remaining_percent <= 0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CreditsSnapshot {
	pub(crate) has_credits: bool,
	pub(crate) unlimited: bool,
	pub(crate) balance: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResetCreditsSnapshot {
	pub(crate) available_count: Option<i64>,
	pub(crate) total_earned_count: Option<i64>,
	pub(crate) credits: Vec<ResetCreditSummary>,
	pub(crate) checked_at_unix_epoch: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCreditSummary {
	pub(crate) granted_at_unix_epoch: Option<i64>,
	pub(crate) expires_at_unix_epoch: Option<i64>,
	pub(crate) status: Option<String>,
}
