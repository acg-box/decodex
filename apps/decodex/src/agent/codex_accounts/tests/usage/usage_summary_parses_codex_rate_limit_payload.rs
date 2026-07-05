use crate::agent::codex_accounts::{self, CreditsSnapshot, UsageWindow};

#[test]
fn usage_summary_parses_codex_rate_limit_payload() {
	let payload = serde_json::json!({
		"plan_type": "pro",
		"rate_limit": {
			"primary_window": {
				"used_percent": 42,
				"limit_window_seconds": 18_000,
				"reset_at": 1_800_018_000
			},
			"secondary_window": {
				"used_percent": 84,
				"limit_window_seconds": 604_800,
				"reset_at": 1_800_604_800
			}
		},
		"credits": {
			"has_credits": true,
			"unlimited": false,
			"balance": "9.99"
		},
		"rate_limit_reached_type": {
			"kind": "workspace_member_credits_depleted"
		}
	});
	let summary = codex_accounts::usage_snapshot_from_payload(&payload, 1_800_000_000);

	assert_eq!(summary.plan_type.as_deref(), Some("pro"));
	assert_eq!(
		summary.primary,
		Some(UsageWindow {
			window_seconds: Some(18_000),
			remaining_percent: 58,
			resets_at_unix_epoch: Some(1_800_018_000),
		})
	);
	assert_eq!(
		summary.secondary,
		Some(UsageWindow {
			window_seconds: Some(604_800),
			remaining_percent: 16,
			resets_at_unix_epoch: Some(1_800_604_800),
		})
	);
	assert_eq!(
		summary.credits,
		Some(CreditsSnapshot {
			has_credits: true,
			unlimited: false,
			balance: Some(String::from("9.99")),
		})
	);
	assert_eq!(
		summary.rate_limit_reached_type.as_deref(),
		Some("workspace_member_credits_depleted")
	);
}
