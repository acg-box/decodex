use crate::agent::codex_accounts::{self};

#[test]
fn profile_summary_falls_back_to_daily_usage_peak() {
	let payload = serde_json::json!({
		"stats": {
			"daily_usage_buckets": [
				{ "start_date": "2026-05-30", "tokens": 123_456 },
				{ "start_date": "2026-05-31", "tokens": 789_000 }
			]
		}
	});
	let summary = codex_accounts::profile_snapshot_from_payload(&payload, 1_800_000_000)
		.expect("profile summary should parse from daily usage buckets");

	assert_eq!(summary.peak_daily_tokens, Some(789_000));
}
