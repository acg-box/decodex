use serde::{Deserialize, Serialize};

use crate::accounts::{
	AccountSummary,
	usage_history::{self, USAGE_ESTIMATE_WINDOW_SECONDS},
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AccountUsageDailySummary {
	pub(crate) date: String,
	pub(crate) used_percent: i64,
	pub(crate) capacity_multiplier: i64,
	pub(crate) checked_at_unix_epoch: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AccountUsageHistoryRecord {
	pub(super) date: String,
	pub(super) account_fingerprint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) email: Option<String>,
	pub(super) used_percent: i64,
	#[serde(default = "usage_history::default_account_capacity_multiplier")]
	pub(super) capacity_multiplier: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) window_seconds: Option<i64>,
	pub(super) checked_at_unix_epoch: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) resets_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) primary_window_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) primary_remaining_percent: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) primary_resets_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) secondary_window_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) secondary_remaining_percent: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) secondary_resets_at_unix_epoch: Option<i64>,
}
impl AccountUsageHistoryRecord {
	pub(super) fn daily_summary(&self) -> AccountUsageDailySummary {
		AccountUsageDailySummary {
			date: self.date.clone(),
			used_percent: self.used_percent,
			capacity_multiplier: usage_history::normalized_account_capacity_multiplier(
				self.capacity_multiplier,
			),
			checked_at_unix_epoch: self.checked_at_unix_epoch,
		}
	}

	pub(super) fn is_recent(&self, now_unix_epoch: i64) -> bool {
		now_unix_epoch.saturating_sub(self.checked_at_unix_epoch) <= USAGE_ESTIMATE_WINDOW_SECONDS
	}

	pub(super) fn matches_account(&self, account: &AccountSummary) -> bool {
		self.account_fingerprint == account.account_fingerprint
			|| self
				.email
				.as_deref()
				.zip(account.email.as_deref())
				.is_some_and(|(left, right)| left == right)
	}

	pub(super) fn same_daily_slot(&self, other: &Self) -> bool {
		self.date == other.date
			&& (self.account_fingerprint == other.account_fingerprint
				|| self
					.email
					.as_deref()
					.zip(other.email.as_deref())
					.is_some_and(|(left, right)| left == right))
	}

	pub(super) fn apply_missing_usage_windows(
		&self,
		account: &mut AccountSummary,
		now_unix_epoch: i64,
	) {
		self.apply_missing_primary_usage_window(account, now_unix_epoch);
		self.apply_missing_secondary_usage_window(account, now_unix_epoch);
	}

	fn apply_missing_primary_usage_window(
		&self,
		account: &mut AccountSummary,
		now_unix_epoch: i64,
	) {
		if usage_history::has_usage_window(
			account.primary_window_seconds,
			account.primary_remaining_percent,
		) || !usage_history::has_current_usage_window(
			self.primary_window_seconds,
			self.primary_remaining_percent,
			self.primary_resets_at_unix_epoch,
			now_unix_epoch,
		) {
			return;
		}

		account.primary_window_seconds = self.primary_window_seconds;
		account.primary_remaining_percent = self.primary_remaining_percent;
		account.primary_resets_at_unix_epoch = self.primary_resets_at_unix_epoch;
	}

	fn apply_missing_secondary_usage_window(
		&self,
		account: &mut AccountSummary,
		now_unix_epoch: i64,
	) {
		let (legacy_window_seconds, legacy_remaining_percent, legacy_resets_at_unix_epoch) =
			if self.legacy_usage_matches_primary_window() {
				(None, None, None)
			} else {
				(
					self.window_seconds,
					Some(usage_history::remaining_percent_from_used(self.used_percent)),
					self.resets_at_unix_epoch,
				)
			};
		let window_seconds = self.secondary_window_seconds.or(legacy_window_seconds);
		let remaining_percent = self.secondary_remaining_percent.or(legacy_remaining_percent);
		let resets_at_unix_epoch =
			self.secondary_resets_at_unix_epoch.or(legacy_resets_at_unix_epoch);

		if usage_history::has_usage_window(
			account.secondary_window_seconds,
			account.secondary_remaining_percent,
		) || !usage_history::has_current_usage_window(
			window_seconds,
			remaining_percent,
			resets_at_unix_epoch,
			now_unix_epoch,
		) {
			return;
		}

		account.secondary_window_seconds = window_seconds;
		account.secondary_remaining_percent = remaining_percent;
		account.secondary_resets_at_unix_epoch = resets_at_unix_epoch;
	}

	fn legacy_usage_matches_primary_window(&self) -> bool {
		self.window_seconds
			.zip(self.primary_window_seconds)
			.is_some_and(|(legacy, primary)| legacy == primary)
			&& self.primary_remaining_percent.is_some_and(|remaining| {
				remaining == usage_history::remaining_percent_from_used(self.used_percent)
			}) && self.resets_at_unix_epoch == self.primary_resets_at_unix_epoch
	}
}
