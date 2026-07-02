use crate::accounts::{AccountSummary, usage_history};

#[derive(Clone, Copy)]
pub(crate) struct SevenDayUsageBasis {
	pub(super) used_percent: i64,
	pub(super) window_seconds: Option<i64>,
	pub(super) resets_at_unix_epoch: Option<i64>,
}
impl SevenDayUsageBasis {
	pub(crate) fn from_account(account: &AccountSummary) -> Option<Self> {
		let secondary = Self::from_window(
			account.secondary_remaining_percent,
			account.secondary_window_seconds,
			account.secondary_resets_at_unix_epoch,
		);

		if let Some(basis) = secondary
			&& usage_history::accepts_secondary_usage_window(basis.window_seconds)
		{
			return Some(basis);
		}

		Self::from_window(
			account.primary_remaining_percent,
			account.primary_window_seconds,
			account.primary_resets_at_unix_epoch,
		)
		.filter(|basis| basis.window_seconds.is_some_and(usage_history::is_seven_day_usage_window))
	}

	fn from_window(
		remaining_percent: Option<i64>,
		window_seconds: Option<i64>,
		resets_at_unix_epoch: Option<i64>,
	) -> Option<Self> {
		Some(Self {
			used_percent: usage_history::used_percent_from_remaining(remaining_percent?),
			window_seconds,
			resets_at_unix_epoch,
		})
	}
}
