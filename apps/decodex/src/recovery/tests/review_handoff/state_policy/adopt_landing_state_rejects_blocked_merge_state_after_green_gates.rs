use std::fs;

use tempfile::TempDir;

use crate::{
	recovery,
	recovery::tests::{self},
};

#[test]
fn adopt_landing_state_rejects_blocked_merge_state_after_green_gates() {
	let mut landing_state = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.merge_state_status = String::from("BLOCKED");

	let error = recovery::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover should not bypass blocked merge state");

	assert!(error.to_string().contains("not ready to adopt"));
	assert!(error.to_string().contains("mergeStateStatus=`BLOCKED`"));
}

#[test]
fn adopt_landing_state_rejects_closed_or_draft_prs() {
	let mut closed = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	closed.state = String::from("CLOSED");

	let error = recovery::validate_adopt_landing_state(&closed)
		.expect_err("manual takeover must reject closed PRs");

	assert!(error.to_string().contains("adopt requires `OPEN`"));

	let mut draft = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	draft.is_draft = true;

	let error = recovery::validate_adopt_landing_state(&draft)
		.expect_err("manual takeover must reject draft PRs");

	assert!(error.to_string().contains("is still draft"));
}

#[test]
fn adopt_landing_state_rejects_failed_required_checks() {
	let mut landing_state = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.status_check_rollup_state = Some(String::from("FAILURE"));
	landing_state.merge_state_status = String::from("BLOCKED");

	let error = recovery::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover must reject failed required checks");

	assert!(error.to_string().contains("failed required checks"));
}

#[test]
fn adopt_existing_worktree_mapping_accepts_same_project_and_path() {
	let temp_dir = TempDir::new().expect("temp worktree should exist");
	let branch_name = "x/pubfi-pub-718";
	let issue = tests::sample_issue("In Progress");
	let mapping = tests::sample_worktree_at(branch_name, temp_dir.path());
	let canonical_worktree =
		fs::canonicalize(temp_dir.path()).expect("temp worktree should canonicalize");

	recovery::validate_adopt_existing_worktree_mapping(
		"pubfi",
		&issue,
		&mapping,
		&canonical_worktree,
	)
	.expect("matching mapping should be accepted");
}

#[test]
fn adopt_existing_worktree_mapping_accepts_stale_branch_for_same_path() {
	let retained_dir = TempDir::new().expect("retained worktree should exist");
	let issue = tests::sample_issue("In Progress");
	let mapping = tests::sample_worktree_at("x/pubfi-pub-718-old", retained_dir.path());
	let retained_worktree =
		fs::canonicalize(retained_dir.path()).expect("retained worktree should canonicalize");

	recovery::validate_adopt_existing_worktree_mapping(
		"pubfi",
		&issue,
		&mapping,
		&retained_worktree,
	)
	.expect("stale mapping branch should be adopted when path matches");
}

#[test]
fn adopt_existing_worktree_mapping_rejects_stale_path() {
	let retained_dir = TempDir::new().expect("retained worktree should exist");
	let current_dir = TempDir::new().expect("current worktree should exist");
	let issue = tests::sample_issue("In Progress");
	let mapping = tests::sample_worktree_at("x/pubfi-pub-718", retained_dir.path());
	let current_worktree =
		fs::canonicalize(current_dir.path()).expect("current worktree should canonicalize");
	let error = recovery::validate_adopt_existing_worktree_mapping(
		"pubfi",
		&issue,
		&mapping,
		&current_worktree,
	)
	.expect_err("stale mapping path must be rejected");

	assert!(error.to_string().contains("already has a retained worktree mapping at"));
}

#[test]
fn manual_adopt_run_id_is_stable_for_head() {
	let head_oid = "0123456789abcdef0123456789abcdef01234567";
	let run_id = recovery::manual_adopt_run_id("XY-944", 2, head_oid);

	assert_eq!(run_id, "xy-944-manual-adopt-2-0123456789ab");
	assert_eq!(run_id, recovery::manual_adopt_run_id("XY-944", 2, head_oid));
}
