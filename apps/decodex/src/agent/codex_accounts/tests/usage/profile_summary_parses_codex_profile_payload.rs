use crate::agent::codex_accounts::{self};

#[test]
fn profile_summary_parses_codex_profile_payload() {
	let payload = serde_json::json!({
		"profile": {
			"display_name": "  Copy Account  ",
			"username": "copy"
		},
		"stats": {
			"lifetime_tokens": 47_200_000_000_i64,
			"peak_daily_tokens": 1_500_000_000_i64,
			"longest_running_turn_sec": 10_080,
			"current_streak_days": 12,
			"longest_streak_days": 68,
			"daily_usage_buckets": [
				{ "start_date": "2026-05-30", "tokens": 123_456 },
				{ "start_date": "2026-05-31", "tokens": 789_000 }
			]
		}
	});
	let summary = codex_accounts::profile_snapshot_from_payload(&payload, 1_800_000_000)
		.expect("profile summary should parse");

	assert_eq!(summary.display_name.as_deref(), Some("Copy Account"));
	assert_eq!(summary.username.as_deref(), Some("copy"));
	assert_eq!(summary.lifetime_tokens, Some(47_200_000_000));
	assert_eq!(summary.peak_daily_tokens, Some(1_500_000_000));
	assert_eq!(summary.longest_task_seconds, Some(10_080));
	assert_eq!(summary.current_streak_days, Some(12));
	assert_eq!(summary.longest_streak_days, Some(68));
	assert_eq!(summary.daily_usage.len(), 2);
	assert_eq!(summary.daily_usage[1].date, "2026-05-31");
	assert_eq!(summary.daily_usage[1].tokens, 789_000);
}
