use serde::Serialize;

use crate::accounts::usage_history::{self, USAGE_ESTIMATE_WINDOW_DAYS};

#[derive(Clone, Serialize)]
pub(crate) struct AccountUsageEstimateSummary {
	pub(crate) window_days: i64,
	pub(crate) account_count: usize,
	pub(crate) account_estimate_count: usize,
	pub(crate) total_capacity_percent: i64,
	pub(crate) total_used_percent: i64,
	pub(crate) total_used_of_capacity_percent: f64,
	pub(crate) average_daily_used_percent: f64,
	pub(crate) average_daily_pool_percent: f64,
}
impl AccountUsageEstimateSummary {
	pub(super) fn new(
		account_count: usize,
		account_estimate_count: usize,
		total_capacity_percent: i64,
		total_used_percent: i64,
	) -> Option<Self> {
		if account_count == 0 || account_estimate_count == 0 {
			return None;
		}

		let total_used_of_capacity_percent =
			usage_history::percent_ratio(total_used_percent, total_capacity_percent);

		Some(Self {
			window_days: USAGE_ESTIMATE_WINDOW_DAYS,
			account_count,
			account_estimate_count,
			total_capacity_percent,
			total_used_percent,
			total_used_of_capacity_percent,
			average_daily_used_percent: total_used_percent as f64
				/ USAGE_ESTIMATE_WINDOW_DAYS as f64,
			average_daily_pool_percent: total_used_of_capacity_percent
				/ USAGE_ESTIMATE_WINDOW_DAYS as f64,
		})
	}
}
