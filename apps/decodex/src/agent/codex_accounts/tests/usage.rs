use time::OffsetDateTime;

use crate::agent::codex_accounts::{
	self, AccountPoolRecord, CodexAccountActivitySummary, CodexTokenData, CreditsSnapshot,
	UsageWindow, usage,
};

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

#[test]
fn usage_limit_detects_depleted_windows_without_credit_heuristics() {
	let payload = serde_json::json!({
		"plan_type": "pro",
		"rate_limit": {
			"primary_window": {
				"used_percent": 0,
				"limit_window_seconds": 18_000,
				"reset_at": 1_800_018_000
			},
			"secondary_window": {
				"used_percent": 100,
				"limit_window_seconds": 604_800,
				"reset_at": 1_800_604_800
			}
		},
		"credits": {
			"has_credits": false,
			"unlimited": false,
			"balance": "0"
		},
		"rate_limit_reached_type": null
	});
	let summary = codex_accounts::usage_snapshot_from_payload(&payload, 1_800_000_000);

	assert_eq!(summary.primary.as_ref().map(|window| window.remaining_percent), Some(100));
	assert_eq!(summary.secondary.as_ref().map(|window| window.remaining_percent), Some(0));
	assert_eq!(summary.credits.as_ref().map(|credits| credits.has_credits), Some(false));
	assert!(summary.is_limited());

	let record = AccountPoolRecord {
		email: Some(String::from("limited@example.com")),
		disabled: false,
		cooldown_until_unix_epoch: None,
		cooldown_until: None,
		last_selected_at_unix_epoch: None,
		auth_failed_at_unix_epoch: None,
		auth_failure: None,
		auth_mode: Some(String::from("chatgpt")),
		openai_api_key: None,
		tokens: Some(CodexTokenData {
			email: None,
			id_token: None,
			access_token: String::from("access"),
			refresh_token: String::from("refresh"),
			account_id: Some(String::from("acct_limited")),
		}),
		last_refresh: None,
	};
	let login = record
		.login_from_usage(summary, "not_needed")
		.expect("limited usage should still produce an account summary");

	assert_eq!(login.summary().status, "usage_limited");

	let available_payload = serde_json::json!({
		"plan_type": "pro",
		"rate_limit": {
			"primary_window": {
				"used_percent": 40,
				"limit_window_seconds": 18_000,
				"reset_at": 1_800_018_000
			},
			"secondary_window": {
				"used_percent": 72,
				"limit_window_seconds": 604_800,
				"reset_at": 1_800_604_800
			}
		},
		"credits": {
			"has_credits": false,
			"unlimited": false,
			"balance": "0"
		},
		"rate_limit_reached_type": null
	});
	let available_summary =
		codex_accounts::usage_snapshot_from_payload(&available_payload, 1_800_000_000);

	assert_eq!(available_summary.primary.as_ref().map(|window| window.remaining_percent), Some(60));
	assert_eq!(
		available_summary.secondary.as_ref().map(|window| window.remaining_percent),
		Some(28)
	);
	assert!(!available_summary.is_limited());
}

#[test]
fn usage_summary_ignores_zero_second_placeholder_windows() {
	let payload = serde_json::json!({
		"plan_type": "pro",
		"rate_limit": {
			"primary_window": {
				"used_percent": 0,
				"limit_window_seconds": 0,
				"reset_at": 1_800_000_000
			},
			"secondary_window": null
		},
		"rate_limit_reached_type": null
	});
	let summary = codex_accounts::usage_snapshot_from_payload(&payload, 1_800_000_000);

	assert_eq!(summary.primary, None);
	assert_eq!(summary.secondary, None);
	assert!(!summary.is_limited());

	let record = AccountPoolRecord {
		email: Some(String::from("placeholder@example.com")),
		disabled: false,
		cooldown_until_unix_epoch: None,
		cooldown_until: None,
		last_selected_at_unix_epoch: None,
		auth_failed_at_unix_epoch: None,
		auth_failure: None,
		auth_mode: Some(String::from("chatgpt")),
		openai_api_key: None,
		tokens: Some(CodexTokenData {
			email: None,
			id_token: None,
			access_token: String::from("access"),
			refresh_token: String::from("refresh"),
			account_id: Some(String::from("acct_placeholder")),
		}),
		last_refresh: None,
	};
	let login = record
		.login_from_usage(summary, "not_needed")
		.expect("placeholder usage should still produce an account summary");

	assert_eq!(login.summary().status, "available");
	assert_eq!(login.summary().primary_window_seconds, None);
	assert_eq!(login.summary().primary_remaining_percent, None);
	assert_eq!(login.summary().primary_resets_at_unix_epoch, None);
}

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
