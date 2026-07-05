use crate::agent::codex_accounts::{self, AccountPoolRecord, CodexTokenData};

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
