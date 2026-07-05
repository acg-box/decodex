use time::OffsetDateTime;

use crate::agent::codex_accounts::{CodexAccountActivitySummary, usage};

#[test]
fn usage_cache_preserves_current_windows_across_placeholder_refresh() {
	let now = OffsetDateTime::now_utc().unix_timestamp();
	let cached = [CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		status: String::from("available"),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(now + 18_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(now + 604_800),
		..CodexAccountActivitySummary::default()
	}];
	let mut refreshed = [CodexAccountActivitySummary {
		account_fingerprint: String::from("...123456"),
		email: Some(String::from("copy@example.com")),
		status: String::from("available"),
		checked_at_unix_epoch: Some(now + 60),
		profile_lifetime_tokens: Some(47_200_000_000),
		..CodexAccountActivitySummary::default()
	}];

	usage::preserve_cached_usage_windows(&mut refreshed, &cached, now + 60);

	assert_eq!(refreshed[0].primary_window_seconds, Some(18_000));
	assert_eq!(refreshed[0].primary_remaining_percent, Some(72));
	assert_eq!(refreshed[0].primary_resets_at_unix_epoch, Some(now + 18_000));
	assert_eq!(refreshed[0].secondary_window_seconds, Some(604_800));
	assert_eq!(refreshed[0].secondary_remaining_percent, Some(91));
	assert_eq!(refreshed[0].secondary_resets_at_unix_epoch, Some(now + 604_800));
	assert_eq!(refreshed[0].profile_lifetime_tokens, Some(47_200_000_000));
}
