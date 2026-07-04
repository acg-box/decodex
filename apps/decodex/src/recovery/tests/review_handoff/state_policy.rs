use std::fs;

use tempfile::TempDir;

use crate::recovery::{
	RebindMode,
	tests::{self},
};

#[test]
fn rebind_state_allows_missing_marker_partial_in_progress_handoff() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Progress");
	let transition = super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::RestoreMissingHandoff,
	)
	.expect("missing-marker rebind should recover partial in-progress handoff")
	.expect("partial in-progress handoff should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_allows_current_marker_partial_in_progress_handoff() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Progress");
	let transition = super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::CompleteExistingHandoffState,
	)
	.expect("current-marker state completion should recover partial in-progress handoff")
	.expect("partial in-progress handoff should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_allows_current_marker_failure_state_drift_recovery() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("Todo");
	let transition = super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::CompleteExistingHandoffState,
	)
	.expect("current-marker state completion should recover failure-state drift")
	.expect("failure-state drift should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_allows_missing_marker_writeback_failure_recovery() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("Todo");
	let transition = super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::RestoreMissingHandoffAfterWritebackFailure,
	)
	.expect("missing-marker writeback failure should recover failure-state drift")
	.expect("failure-state writeback recovery should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_rejects_failure_state_without_current_marker_repair_mode() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("Todo");

	for mode in [RebindMode::RestoreMissingHandoff, RebindMode::RefreshExistingHandoff] {
		let error = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			mode,
		)
		.expect_err("failure-state repair requires current-marker completion mode");

		assert!(
			error.to_string().contains("review handoff rebind requires"),
			"unexpected error for {mode:?}: {error}"
		);
	}
}

#[test]
fn rebind_state_requires_success_state_for_existing_marker_refresh() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Progress");
	let error = super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::RefreshExistingHandoff,
	)
	.expect_err("existing-marker refresh should still require success state");

	assert!(error.to_string().contains("requires `In Review`"));
	assert!(!error.to_string().contains("partial missing-marker"));
}

#[test]
fn adopt_state_allows_in_progress_or_review_only() {
	let workflow = tests::sample_workflow();
	let in_progress = tests::sample_issue("In Progress");
	let transition = super::validate_adopt_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&in_progress,
	)
	.expect("in-progress issue should be adoptable")
	.expect("in-progress issue should transition to review");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");

	let in_review = tests::sample_issue("In Review");
	let no_transition =
		super::validate_adopt_issue_state_for_policy(workflow.frontmatter().tracker(), &in_review)
			.expect("in-review issue should remain adoptable");

	assert!(no_transition.is_none());

	let todo = tests::sample_issue("Todo");
	let error =
		super::validate_adopt_issue_state_for_policy(workflow.frontmatter().tracker(), &todo)
			.expect_err("manual takeover should not bypass failure/start states");

	assert!(error.to_string().contains("manual takeover adopt requires"));
}

#[test]
fn adopt_landing_state_rejects_pending_checks() {
	let mut landing_state = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.status_check_rollup_state = Some(String::from("PENDING"));

	let error = super::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover must not adopt pending checks");

	assert!(error.to_string().contains("still waiting on checks"));
}

#[test]
fn adopt_landing_state_rejects_blocked_merge_state_after_green_gates() {
	let mut landing_state = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.merge_state_status = String::from("BLOCKED");

	let error = super::validate_adopt_landing_state(&landing_state)
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

	let error = super::validate_adopt_landing_state(&closed)
		.expect_err("manual takeover must reject closed PRs");

	assert!(error.to_string().contains("adopt requires `OPEN`"));

	let mut draft = tests::sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	draft.is_draft = true;

	let error = super::validate_adopt_landing_state(&draft)
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

	let error = super::validate_adopt_landing_state(&landing_state)
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

	super::validate_adopt_existing_worktree_mapping("pubfi", &issue, &mapping, &canonical_worktree)
		.expect("matching mapping should be accepted");
}

#[test]
fn adopt_existing_worktree_mapping_accepts_stale_branch_for_same_path() {
	let retained_dir = TempDir::new().expect("retained worktree should exist");
	let issue = tests::sample_issue("In Progress");
	let mapping = tests::sample_worktree_at("x/pubfi-pub-718-old", retained_dir.path());
	let retained_worktree =
		fs::canonicalize(retained_dir.path()).expect("retained worktree should canonicalize");

	super::validate_adopt_existing_worktree_mapping("pubfi", &issue, &mapping, &retained_worktree)
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
	let error = super::validate_adopt_existing_worktree_mapping(
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
	let run_id = super::manual_adopt_run_id("XY-944", 2, head_oid);

	assert_eq!(run_id, "xy-944-manual-adopt-2-0123456789ab");
	assert_eq!(run_id, super::manual_adopt_run_id("XY-944", 2, head_oid));
}
