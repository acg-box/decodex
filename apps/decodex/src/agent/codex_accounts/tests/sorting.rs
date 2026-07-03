use std::fs;

use tempfile::TempDir;

use crate::agent::codex_accounts::{
	CodexAccountActivitySummary, CodexAccountLogin, CodexAccountPool, CodexAccountProvider,
	DEFAULT_REFRESH_ENDPOINT, compare_account_candidates,
};

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
		super::codex_account_login_for_sort("acct_primary_rich", Some(100), Some(40), None),
		super::codex_account_login_for_sort("acct_balanced", Some(80), Some(80), None),
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
	let usage_endpoint = super::start_codex_usage_fixture_server(vec![
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
