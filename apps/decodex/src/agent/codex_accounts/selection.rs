use std::cmp::Ordering;

use crate::{agent::codex_accounts::CodexAccountLogin, state::CodexAccountActivitySummary};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AccountCandidateScore {
	not_limited: bool,
	bottleneck_remaining_percent: i64,
	combined_remaining_score: i64,
	primary_remaining_percent: i64,
	secondary_remaining_percent: i64,
}

pub(super) fn compare_account_candidates(
	left: &CodexAccountLogin,
	right: &CodexAccountLogin,
) -> Ordering {
	account_candidate_score(right)
		.cmp(&account_candidate_score(left))
		.then_with(|| {
			left.last_selected_at_unix_epoch
				.unwrap_or(0)
				.cmp(&right.last_selected_at_unix_epoch.unwrap_or(0))
		})
		.then_with(|| left.summary.account_fingerprint.cmp(&right.summary.account_fingerprint))
}

pub(super) fn account_summary_is_limited(summary: &CodexAccountActivitySummary) -> bool {
	summary.rate_limit_reached_type.is_some()
		|| summary.status.to_lowercase().contains("limit")
		|| summary.primary_remaining_percent == Some(0)
		|| summary.secondary_remaining_percent == Some(0)
}

pub(super) fn account_summaries(
	selected: &CodexAccountLogin,
	candidates: &[CodexAccountLogin],
) -> Vec<CodexAccountActivitySummary> {
	let mut summaries = Vec::with_capacity(candidates.len() + 1);

	summaries.push(selected.summary().clone());
	summaries.extend(candidates.iter().map(|candidate| candidate.summary().clone()));

	summaries
}

fn account_candidate_score(candidate: &CodexAccountLogin) -> AccountCandidateScore {
	let summary = candidate.summary();
	let primary = summary
		.primary_remaining_percent
		.unwrap_or_else(|| summary.secondary_remaining_percent.unwrap_or(0));
	let secondary = summary.secondary_remaining_percent.unwrap_or(primary);

	AccountCandidateScore {
		not_limited: !account_summary_is_limited(summary),
		bottleneck_remaining_percent: primary.min(secondary),
		combined_remaining_score: primary.saturating_mul(secondary),
		primary_remaining_percent: primary,
		secondary_remaining_percent: secondary,
	}
}
