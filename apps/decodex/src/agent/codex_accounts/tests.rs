use std::{
	fs,
	io::{Read as _, Write as _},
	net::TcpListener,
	thread,
};

use serde_json::Value;
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::agent::codex_accounts::{
	self, AccountPoolRecord, CodexAccountActivitySummary, CodexAccountAuthFailure,
	CodexAccountLogin, CodexAccountPool, CodexAccountProvider, CodexTokenData, CreditsSnapshot,
	DEFAULT_REFRESH_ENDPOINT, Path, ProactiveRefreshReason, UsageWindow,
	compare_account_candidates, record, usage,
};

#[test]
fn accounts_accept_flat_and_wrapped_auth_jsonl_records() {
	let input = r#"
		{"email":"primary@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id","access_token":"access","refresh_token":"refresh","account_id":"acct_primary"}}
		{"auth":{"auth_mode":"chatgpt","tokens":{"id_token":"x.eyJlbWFpbCI6IndyYXBwZWRAZXhhbXBsZS5jb20ifQ.y","access_token":"access-2","refresh_token":"refresh-2","account_id":"acct_wrapped"}}}
	"#;
	let records = record::parse_account_records(input, Path::new("/tmp/accounts.jsonl"))
		.expect("records should parse");

	assert_eq!(records.len(), 2);
	assert_eq!(records[0].account_id(), Some("acct_primary"));
	assert_eq!(records[0].email().as_deref(), Some("primary@example.com"));
	assert_eq!(records[1].account_id(), Some("acct_wrapped"));
	assert_eq!(records[1].email().as_deref(), Some("wrapped@example.com"));
}

#[test]
fn account_selector_matches_email_full_id_and_fingerprint() {
	let record = AccountPoolRecord {
		email: Some(String::from("selected@example.com")),
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
			account_id: Some(String::from("acct_fixed_123456")),
		}),
		last_refresh: None,
	};

	assert!(record.matches_account_selector("selected@example.com"));
	assert!(record.matches_account_selector("acct_fixed_123456"));
	assert!(record.matches_account_selector("...123456"));
	assert!(!record.matches_account_selector("other@example.com"));
}

#[test]
fn fixed_account_selection_uses_configured_account_without_balancing() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":85},"secondary_window":{"used_percent":90}}}"#,
	]);

	fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
{"email":"copy@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-copy","refresh_token":"refresh-copy","account_id":"acct_copy"}}
"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		DEFAULT_REFRESH_ENDPOINT,
		Some("copy@example.com"),
	)
	.expect("account pool should initialize");
	let account = pool.select_account().expect("fixed account should select");

	assert_eq!(account.account_id(), "acct_copy");
	assert_eq!(account.summary().email.as_deref(), Some("copy@example.com"));
	assert_eq!(account.account_summaries().len(), 1);
}

#[test]
fn account_activity_snapshot_uses_configured_records_without_usage_probe() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");

	fs::write(
		&accounts_path,
		r#"{"email":"snapshot@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-snapshot","refresh_token":"refresh-snapshot","account_id":"acct_snapshot"}}"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		"http://127.0.0.1:9/usage",
		DEFAULT_REFRESH_ENDPOINT,
		None,
	)
	.expect("account pool should initialize");
	let summaries = pool.account_activity_summaries_snapshot().expect("snapshot should load");

	assert_eq!(summaries.len(), 1);
	assert_eq!(summaries[0].email.as_deref(), Some("snapshot@example.com"));
	assert_eq!(summaries[0].status, "available");
	assert_eq!(summaries[0].refresh_status, "not_checked");
	assert_eq!(summaries[0].primary_remaining_percent, None);
	assert_eq!(summaries[0].note.as_deref(), Some("configured account"));
}

#[test]
fn selection_marks_refresh_auth_failure_and_selects_next_account() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let refresh_endpoint = start_codex_refresh_status_fixture_server(vec![(
		401,
		"Unauthorized",
		r#"{"error":"invalid refresh token"}"#,
	)]);
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
	]);

	fs::write(
		&accounts_path,
		r#"{"email":"bad@example.com","auth_mode":"chatgpt","tokens":{"access_token":"x.eyJleHAiOjEwMDB9.y","refresh_token":"refresh-bad","account_id":"acct_bad"}}
{"email":"good@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-good","refresh_token":"refresh-good","account_id":"acct_good"}}
"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		refresh_endpoint,
		None,
	)
	.expect("account pool should initialize");
	let account = pool.select_account().expect("healthy fallback account should select");

	assert_eq!(account.account_id(), "acct_good");

	let records = fs::read_to_string(&accounts_path).expect("accounts should read");

	assert!(records.contains(r#""auth_failed_at_unix_epoch":"#));
	assert!(records.contains("HTTP 401 Unauthorized"));

	let summaries = pool.account_activity_summaries_snapshot().expect("snapshot should load");
	let bad_summary = summaries
		.iter()
		.find(|summary| summary.email.as_deref() == Some("bad@example.com"))
		.expect("bad account summary should exist");

	assert_eq!(bad_summary.status, "auth_failed");
	assert_eq!(bad_summary.refresh_status, "auth_failed");
	assert!(bad_summary.note.as_deref().is_some_and(|note| note.contains("HTTP 401")));
}

#[test]
fn refresh_account_marks_auth_failure_and_returns_typed_error() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let refresh_endpoint = start_codex_refresh_status_fixture_server(vec![(
		401,
		"Unauthorized",
		r#"{"error":"invalid refresh token"}"#,
	)]);
	let usage_endpoint = start_codex_usage_fixture_server(Vec::new());

	fs::write(
		&accounts_path,
		r#"{"email":"bad@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-bad","refresh_token":"refresh-bad","account_id":"acct_bad"}}"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		refresh_endpoint,
		None,
	)
	.expect("account pool should initialize");
	let error = match pool.refresh_account(Some("acct_bad")) {
		Ok(_) => panic!("refresh auth failure should surface"),
		Err(error) => error,
	};

	assert!(error.downcast_ref::<CodexAccountAuthFailure>().is_some());

	let records = fs::read_to_string(&accounts_path).expect("accounts should read");

	assert!(records.contains(r#""auth_failed_at_unix_epoch":"#));
	assert!(records.contains("HTTP 401 Unauthorized"));
}

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

#[test]
fn proactive_refresh_prefers_access_token_expiration_then_last_refresh() {
	let mut record = AccountPoolRecord {
		email: Some(String::from("refresh@example.com")),
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
			access_token: String::from("x.eyJleHAiOjEwMDB9.y"),
			refresh_token: String::from("refresh"),
			account_id: Some(String::from("acct_refresh")),
		}),
		last_refresh: Some(String::from("2099-01-01T00:00:00Z")),
	};

	assert_eq!(
		record.proactive_refresh_reason(1_001),
		Some(ProactiveRefreshReason::AccessTokenExpired)
	);

	record.tokens.as_mut().expect("tokens should exist").access_token = String::from("opaque");
	record.last_refresh = Some(String::from("2026-01-01T00:00:00Z"));

	assert_eq!(
		record.proactive_refresh_reason(1_768_000_000),
		Some(ProactiveRefreshReason::LastRefreshStale)
	);
}

#[test]
fn token_refresh_syncs_matching_codex_auth_json() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let codex_auth_path = temp_dir.path().join(".codex/auth.json");
	let refresh_endpoint = start_codex_refresh_fixture_server(vec![
		r#"{"id_token":"id-new","access_token":"access-new","refresh_token":"refresh-new"}"#,
	]);
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
	]);

	fs::write(
		&accounts_path,
		r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-old","access_token":"access-old-pool","refresh_token":"refresh-old-pool","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("accounts fixture should write");
	fs::create_dir_all(codex_auth_path.parent().expect("auth path should have parent"))
		.expect("auth parent should create");
	fs::write(
		&codex_auth_path,
		r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-current","access_token":"access-current","refresh_token":"refresh-current","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("Codex auth fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account_and_codex_auth_path(
		&accounts_path,
		usage_endpoint,
		refresh_endpoint,
		None,
		codex_auth_path.clone(),
	)
	.expect("account pool should initialize");
	let account = pool.refresh_account(Some("acct_sync")).expect("account should refresh");

	assert_eq!(account.access_token(), "access-new");

	let accounts_input = fs::read_to_string(&accounts_path).expect("accounts should read");

	assert!(accounts_input.contains(r#""access_token":"access-new""#));
	assert!(accounts_input.contains(r#""refresh_token":"refresh-new""#));

	let codex_auth = fs::read_to_string(&codex_auth_path).expect("Codex auth should read");
	let codex_auth_json =
		serde_json::from_str::<Value>(&codex_auth).expect("Codex auth should parse");

	assert_eq!(codex_auth_json["tokens"]["account_id"], "acct_sync");
	assert_eq!(codex_auth_json["tokens"]["id_token"], "id-new");
	assert_eq!(codex_auth_json["tokens"]["access_token"], "access-new");
	assert_eq!(codex_auth_json["tokens"]["refresh_token"], "refresh-new");
}

#[test]
fn token_refresh_leaves_nonmatching_codex_auth_json_unchanged() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let codex_auth_path = temp_dir.path().join(".codex/auth.json");
	let refresh_endpoint = start_codex_refresh_fixture_server(vec![
		r#"{"id_token":"id-new","access_token":"access-new","refresh_token":"refresh-new"}"#,
	]);
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#,
	]);

	fs::write(
		&accounts_path,
		r#"{"email":"sync@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-old","access_token":"access-old-pool","refresh_token":"refresh-old-pool","account_id":"acct_sync"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("accounts fixture should write");
	fs::create_dir_all(codex_auth_path.parent().expect("auth path should have parent"))
		.expect("auth parent should create");
	fs::write(
		&codex_auth_path,
		r#"{"email":"other@example.com","auth_mode":"chatgpt","tokens":{"id_token":"id-other","access_token":"access-other","refresh_token":"refresh-other","account_id":"acct_other"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
	)
	.expect("Codex auth fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account_and_codex_auth_path(
		&accounts_path,
		usage_endpoint,
		refresh_endpoint,
		None,
		codex_auth_path.clone(),
	)
	.expect("account pool should initialize");
	let account = pool.refresh_account(Some("acct_sync")).expect("account should refresh");

	assert_eq!(account.access_token(), "access-new");

	let codex_auth = fs::read_to_string(&codex_auth_path).expect("Codex auth should read");
	let codex_auth_json =
		serde_json::from_str::<Value>(&codex_auth).expect("Codex auth should parse");

	assert_eq!(codex_auth_json["tokens"]["account_id"], "acct_other");
	assert_eq!(codex_auth_json["tokens"]["id_token"], "id-other");
	assert_eq!(codex_auth_json["tokens"]["access_token"], "access-other");
	assert_eq!(codex_auth_json["tokens"]["refresh_token"], "refresh-other");
}

#[test]
fn account_candidate_sort_prefers_remaining_usage() {
	let mut candidates = [
		CodexAccountLogin {
			access_token: String::from("a"),
			account_id: String::from("acct_a"),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch: None,
			summary: CodexAccountActivitySummary {
				account_fingerprint: String::from("...acct_a"),
				primary_remaining_percent: Some(10),
				secondary_remaining_percent: Some(90),
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		},
		CodexAccountLogin {
			access_token: String::from("b"),
			account_id: String::from("acct_b"),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch: None,
			summary: CodexAccountActivitySummary {
				account_fingerprint: String::from("...acct_b"),
				primary_remaining_percent: Some(70),
				secondary_remaining_percent: Some(40),
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		},
	];

	candidates.sort_by(compare_account_candidates);

	assert_eq!(candidates[0].account_id(), "acct_b");
}

#[test]
fn account_candidate_sort_balances_primary_and_secondary_windows() {
	let mut candidates = [
		codex_account_login_for_sort("acct_primary_rich", Some(100), Some(40), None),
		codex_account_login_for_sort("acct_balanced", Some(80), Some(80), None),
	];

	candidates.sort_by(compare_account_candidates);

	assert_eq!(candidates[0].account_id(), "acct_balanced");
}

#[test]
fn account_candidate_sort_does_not_penalize_zero_credits_when_windows_available() {
	let mut candidates = [
		CodexAccountLogin {
			access_token: String::from("a"),
			account_id: String::from("acct_a"),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch: None,
			summary: CodexAccountActivitySummary {
				account_fingerprint: String::from("...acct_a"),
				primary_remaining_percent: Some(86),
				secondary_remaining_percent: Some(97),
				credits_has_credits: Some(true),
				credits_unlimited: Some(false),
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		},
		CodexAccountLogin {
			access_token: String::from("b"),
			account_id: String::from("acct_b"),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch: None,
			summary: CodexAccountActivitySummary {
				account_fingerprint: String::from("...acct_b"),
				primary_remaining_percent: Some(100),
				secondary_remaining_percent: Some(100),
				credits_has_credits: Some(false),
				credits_unlimited: Some(false),
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		},
	];

	candidates.sort_by(compare_account_candidates);

	assert_eq!(candidates[0].account_id(), "acct_b");
}

#[test]
fn account_candidate_sort_balances_tied_usage_by_last_selection() {
	let mut candidates = [
		CodexAccountLogin {
			access_token: String::from("a"),
			account_id: String::from("acct_a"),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch: Some(20),
			summary: CodexAccountActivitySummary {
				account_fingerprint: String::from("...acct_a"),
				primary_remaining_percent: Some(70),
				secondary_remaining_percent: Some(40),
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		},
		CodexAccountLogin {
			access_token: String::from("b"),
			account_id: String::from("acct_b"),
			plan_type: Some(String::from("pro")),
			last_selected_at_unix_epoch: Some(10),
			summary: CodexAccountActivitySummary {
				account_fingerprint: String::from("...acct_b"),
				primary_remaining_percent: Some(70),
				secondary_remaining_percent: Some(40),
				..CodexAccountActivitySummary::default()
			},
			account_summaries: Vec::new(),
		},
	];

	candidates.sort_by(compare_account_candidates);

	assert_eq!(candidates[0].account_id(), "acct_b");
}

#[test]
fn account_pool_rotates_equal_full_usage_across_dispatches() {
	const FULL_USAGE: &str = r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":0}}}"#;

	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE, FULL_USAGE,
		FULL_USAGE, FULL_USAGE,
	]);

	fs::write(
		&accounts_path,
		r#"{"email":"a@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-a","refresh_token":"refresh-a","account_id":"acct_a"}}
{"email":"b@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-b","refresh_token":"refresh-b","account_id":"acct_b"}}
{"email":"c@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-c","refresh_token":"refresh-c","account_id":"acct_c"}}
"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		DEFAULT_REFRESH_ENDPOINT,
		None,
	)
	.expect("account pool should initialize");
	let selected = (0..3)
		.map(|_| {
			pool.select_account().expect("full usage account should select").account_id().to_owned()
		})
		.collect::<Vec<_>>();

	assert_eq!(selected, vec!["acct_a", "acct_b", "acct_c"]);
}

fn codex_account_login_for_sort(
	account_id: &str,
	primary_remaining_percent: Option<i64>,
	secondary_remaining_percent: Option<i64>,
	last_selected_at_unix_epoch: Option<i64>,
) -> CodexAccountLogin {
	CodexAccountLogin {
		access_token: String::from("access"),
		account_id: account_id.to_owned(),
		plan_type: Some(String::from("pro")),
		last_selected_at_unix_epoch,
		summary: CodexAccountActivitySummary {
			account_fingerprint: format!("...{account_id}"),
			primary_remaining_percent,
			secondary_remaining_percent,
			..CodexAccountActivitySummary::default()
		},
		account_summaries: Vec::new(),
	}
}

fn start_codex_usage_fixture_server(responses: Vec<&'static str>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		for body in responses {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture should accept request");
			let mut buffer = [0_u8; 4_096];
			let _bytes_read = stream.read(&mut buffer).expect("request should read");
			let response = format!(
				"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("usage response should write");
		}
	});

	format!("http://{address}/usage")
}

fn start_codex_refresh_fixture_server(responses: Vec<&'static str>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("refresh fixture should bind");
	let address = listener.local_addr().expect("refresh fixture address should resolve");

	thread::spawn(move || {
		for body in responses {
			let (mut stream, _peer) =
				listener.accept().expect("refresh fixture should accept request");
			let mut buffer = [0_u8; 4_096];
			let _bytes_read = stream.read(&mut buffer).expect("refresh request should read");
			let response = format!(
				"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("refresh response should write");
		}
	});

	format!("http://{address}/oauth/token")
}

fn start_codex_refresh_status_fixture_server(
	responses: Vec<(u16, &'static str, &'static str)>,
) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("refresh fixture should bind");
	let address = listener.local_addr().expect("refresh fixture address should resolve");

	thread::spawn(move || {
		for (status, reason, body) in responses {
			let (mut stream, _peer) =
				listener.accept().expect("refresh fixture should accept request");
			let mut buffer = [0_u8; 4_096];
			let _bytes_read = stream.read(&mut buffer).expect("refresh request should read");
			let response = format!(
				"HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("refresh response should write");
		}
	});

	format!("http://{address}/oauth/token")
}
