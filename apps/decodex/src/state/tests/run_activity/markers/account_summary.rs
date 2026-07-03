use std::{fs, process, slice};

use tempfile::TempDir;

use crate::state::{
	self, CodexAccountActivitySummary, CodexAccountMarker, RUN_ACTIVITY_MARKER_FILE,
};

pub(super) fn assert_run_activity_marker_round_trips_account_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let body = fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
		.expect("marker body should read");

	assert!(body.contains("account="));
	assert!(body.contains("accounts="));
	assert!(!body.contains("codex_account="));
	assert!(!body.contains("codex_accounts="));
}

pub(super) fn assert_run_activity_marker_preserves_account_summary_after_activity_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");
	state::write_run_activity_marker_at(
		temp_dir.path(),
		"run-1",
		1,
		process::id(),
		1_800_000_020,
		Some(1_800_000_019),
	)
	.expect("activity refresh should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let leftover_temp_marker = fs::read_dir(temp_dir.path())
		.expect("tempdir should be readable")
		.filter_map(|entry| entry.ok())
		.any(|entry| entry.file_name().to_string_lossy().contains(".decodex-run-activity."));

	assert!(!leftover_temp_marker, "atomic marker rewrites should not leave temp files");
}

pub(super) fn assert_run_activity_marker_preserves_account_summary_after_stale_rewrite() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("initial activity marker should write");

	let stale_activity_marker = state::read_run_activity_marker_record(temp_dir.path())
		.expect("activity marker should read")
		.expect("activity marker should exist");

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");
	state::write_run_activity_marker_record(temp_dir.path(), &stale_activity_marker)
		.expect("stale activity marker rewrite should preserve current account");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));
}

fn sample_codex_account_activity_summary() -> CodexAccountActivitySummary {
	CodexAccountActivitySummary {
		account_fingerprint: String::from("acct_...cdef"),
		email: Some(String::from("account@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("selected"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_800_000_010),
		selected_at_unix_epoch: Some(1_800_000_011),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(1_800_018_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(1_800_604_800),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("9.99")),
		rate_limit_reached_type: None,
		cooldown_until_unix_epoch: None,
		note: Some(String::from("usage probe ok")),
		..CodexAccountActivitySummary::default()
	}
}
