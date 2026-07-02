use std::path::Path;

use crate::{
	accounts::{
		AccountListResponse, AccountSummary,
		usage_history::{
			self, AccountUsageEstimateSummary, AccountUsageHistory, AccountUsageHistoryRecord,
			SevenDayUsageBasis, USAGE_ESTIMATE_WINDOW_DAYS,
		},
	},
	agent::CodexAccountPool,
	prelude::Result,
	state::CodexAccountActivitySummary,
};

impl AccountListResponse {
	pub(in crate::accounts) fn hydrate_usage_from_path(
		&mut self,
		accounts_path: &Path,
		force_refresh: bool,
	) {
		if self.accounts.is_empty() {
			return;
		}

		match CodexAccountPool::from_accounts_path(accounts_path)
			.and_then(|pool| pool.account_activity_summaries_cached(force_refresh))
		{
			Ok(summaries) => {
				self.apply_usage_summaries(&summaries);

				if let Err(error) = self.refresh_usage_records(accounts_path) {
					self.usage_probe_error = Some(error.to_string());
				}
			},
			Err(error) => self.usage_probe_error = Some(error.to_string()),
		}
	}

	pub(in crate::accounts) fn apply_usage_summaries(
		&mut self,
		summaries: &[CodexAccountActivitySummary],
	) {
		for account in &mut self.accounts {
			if let Some(summary) = matching_usage_summary(account, summaries) {
				account.apply_usage_summary(summary);
			}
		}

		self.refresh_usage_estimate();
	}

	pub(in crate::accounts) fn refresh_usage_records(
		&mut self,
		accounts_path: &Path,
	) -> Result<()> {
		let history_path = usage_history::usage_history_path(accounts_path)?;
		let mut history = AccountUsageHistory::read(&history_path)?;

		history
			.merge_current_records(self.accounts.iter().filter_map(AccountSummary::usage_record));
		history.write(&history_path)?;
		history.apply_to_accounts(&mut self.accounts);
		self.refresh_usage_estimate();

		Ok(())
	}

	fn refresh_usage_estimate(&mut self) {
		let account_count = self.accounts.len();
		let account_estimate_count =
			self.accounts.iter().filter(|account| account.seven_day_used_percent.is_some()).count();
		let total_capacity_percent =
			self.accounts.iter().map(AccountSummary::capacity_percent).sum::<i64>();
		let total_used_percent =
			self.accounts.iter().map(AccountSummary::used_capacity_percent).sum::<i64>();

		self.usage_estimate = AccountUsageEstimateSummary::new(
			account_count,
			account_estimate_count,
			total_capacity_percent,
			total_used_percent,
		);
	}
}

impl AccountSummary {
	fn apply_usage_summary(&mut self, summary: &CodexAccountActivitySummary) {
		self.status = summary.status.clone();
		self.plan_type = summary.plan_type.clone();
		self.capacity_multiplier =
			usage_history::account_capacity_multiplier(self.plan_type.as_deref());
		self.refresh_status = Some(summary.refresh_status.clone());
		self.checked_at_unix_epoch = summary.checked_at_unix_epoch;
		self.primary_window_seconds = summary.primary_window_seconds;
		self.primary_remaining_percent = summary.primary_remaining_percent;
		self.primary_resets_at_unix_epoch = summary.primary_resets_at_unix_epoch;
		self.secondary_window_seconds = summary.secondary_window_seconds;
		self.secondary_remaining_percent = summary.secondary_remaining_percent;
		self.secondary_resets_at_unix_epoch = summary.secondary_resets_at_unix_epoch;
		self.credits_has_credits = summary.credits_has_credits;
		self.credits_unlimited = summary.credits_unlimited;

		self.credits_balance.clone_from(&summary.credits_balance);
		self.rate_limit_reached_type.clone_from(&summary.rate_limit_reached_type);
		self.profile_display_name.clone_from(&summary.profile_display_name);
		self.profile_username.clone_from(&summary.profile_username);

		self.profile_checked_at_unix_epoch = summary.profile_checked_at_unix_epoch;
		self.profile_lifetime_tokens = summary.profile_lifetime_tokens;
		self.profile_peak_daily_tokens = summary.profile_peak_daily_tokens;
		self.profile_longest_task_seconds = summary.profile_longest_task_seconds;
		self.profile_current_streak_days = summary.profile_current_streak_days;
		self.profile_longest_streak_days = summary.profile_longest_streak_days;

		self.profile_daily_usage.clone_from(&summary.profile_daily_usage);

		if summary.cooldown_until_unix_epoch.is_some() {
			self.cooldown_until_unix_epoch = summary.cooldown_until_unix_epoch;
		}

		self.note.clone_from(&summary.note);

		self.recovery_action = usage_history::account_recovery_action(
			self.status.as_str(),
			self.refresh_token_present,
			self.refresh_status.as_deref(),
			self.note.as_deref(),
		);

		self.apply_usage_estimate();
	}

	fn apply_usage_estimate(&mut self) {
		let Some(basis) = SevenDayUsageBasis::from_account(self) else {
			self.seven_day_used_percent = None;
			self.seven_day_daily_average_percent = None;

			return;
		};

		self.seven_day_used_percent = Some(basis.used_percent);
		self.seven_day_daily_average_percent =
			Some(basis.used_percent as f64 / USAGE_ESTIMATE_WINDOW_DAYS as f64);
	}

	fn usage_record(&self) -> Option<AccountUsageHistoryRecord> {
		let basis = SevenDayUsageBasis::from_account(self)?;
		let checked_at_unix_epoch = self.checked_at_unix_epoch?;

		Some(AccountUsageHistoryRecord {
			date: usage_history::usage_record_date(checked_at_unix_epoch)?,
			account_fingerprint: self.account_fingerprint.clone(),
			email: self.email.clone(),
			used_percent: basis.used_percent,
			capacity_multiplier: self.capacity_multiplier,
			window_seconds: basis.window_seconds,
			checked_at_unix_epoch,
			resets_at_unix_epoch: basis.resets_at_unix_epoch,
			primary_window_seconds: self.primary_window_seconds,
			primary_remaining_percent: self.primary_remaining_percent,
			primary_resets_at_unix_epoch: self.primary_resets_at_unix_epoch,
			secondary_window_seconds: self.secondary_window_seconds,
			secondary_remaining_percent: self.secondary_remaining_percent,
			secondary_resets_at_unix_epoch: self.secondary_resets_at_unix_epoch,
		})
	}

	fn capacity_percent(&self) -> i64 {
		usage_history::normalized_account_capacity_multiplier(self.capacity_multiplier)
			.saturating_mul(100)
	}

	fn used_capacity_percent(&self) -> i64 {
		self.seven_day_used_percent.unwrap_or_default().saturating_mul(
			usage_history::normalized_account_capacity_multiplier(self.capacity_multiplier),
		)
	}
}

fn matching_usage_summary<'a>(
	account: &AccountSummary,
	summaries: &'a [CodexAccountActivitySummary],
) -> Option<&'a CodexAccountActivitySummary> {
	summaries.iter().find(|summary| {
		account
			.email
			.as_deref()
			.zip(summary.email.as_deref())
			.is_some_and(|(account_email, summary_email)| account_email == summary_email)
			|| account.account_fingerprint == summary.account_fingerprint
	})
}
