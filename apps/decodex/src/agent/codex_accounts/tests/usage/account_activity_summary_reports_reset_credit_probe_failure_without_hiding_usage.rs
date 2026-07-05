use std::fs;

use tempfile::TempDir;

use crate::agent::codex_accounts::{
	CodexAccountPool, DEFAULT_REFRESH_ENDPOINT,
};

#[test]
fn account_activity_summary_reports_reset_credit_probe_failure_without_hiding_usage() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let accounts_path = temp_dir.path().join("accounts.jsonl");
	let usage_endpoint = super::super::start_codex_usage_fixture_server(vec![
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":0},"secondary_window":{"used_percent":20}}}"#,
	]);
	let (reset_credits_endpoint, reset_requests) =
		super::super::start_codex_status_fixture_server_with_request_capture(
			"/reset-credits",
			vec![(401, "Unauthorized", r#"{"error":"bad token"}"#)],
		);

	fs::write(
		&accounts_path,
		r#"{"email":"reset-failure@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-reset-failure","refresh_token":"refresh-reset-failure","account_id":"acct_reset_failure"}}"#,
	)
	.expect("accounts fixture should write");

	let pool = CodexAccountPool::new_with_fixed_account(
		&accounts_path,
		usage_endpoint,
		reset_credits_endpoint,
		DEFAULT_REFRESH_ENDPOINT,
		None,
	)
	.expect("account pool should initialize");
	let summaries = pool
		.account_activity_summaries_cached(true)
		.expect("activity summaries should preserve usage while surfacing reset failure");

	assert_eq!(summaries.len(), 1);
	assert_eq!(summaries[0].status, "available");
	assert_eq!(summaries[0].primary_remaining_percent, Some(100));
	assert_eq!(summaries[0].secondary_remaining_percent, Some(80));
	assert_eq!(summaries[0].reset_credits_available_count, None);
	assert!(summaries[0].reset_credits.is_empty());
	assert_eq!(
		summaries[0].note.as_deref(),
		Some(
			"reset credits probe failed: reset credits endpoint returned 401; credentials may be expired or the Authorization header may be missing or invalid"
		)
	);

	let request = reset_requests.recv().expect("reset credits request should be captured");
	let request_lowercase = request.to_ascii_lowercase();

	assert!(request.starts_with("GET /reset-credits "));
	assert!(request_lowercase.contains("authorization: bearer access-reset-failure"));
	assert!(request_lowercase.contains("chatgpt-account-id: acct_reset_failure"));
}
