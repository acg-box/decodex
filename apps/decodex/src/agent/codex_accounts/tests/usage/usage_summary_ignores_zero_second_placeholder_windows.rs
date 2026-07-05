use crate::agent::codex_accounts::{self, AccountPoolRecord, CodexTokenData};

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
