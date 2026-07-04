use crate::{agent::codex_accounts::selection, state::CodexAccountActivitySummary};

pub(crate) fn preserve_cached_usage_windows(
	summaries: &mut [CodexAccountActivitySummary],
	cached_summaries: &[CodexAccountActivitySummary],
	now_unix_epoch: i64,
) {
	for summary in summaries {
		if selection::account_summary_is_limited(summary) {
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
