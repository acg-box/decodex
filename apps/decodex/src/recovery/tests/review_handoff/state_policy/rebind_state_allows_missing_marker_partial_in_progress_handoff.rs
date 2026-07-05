use crate::recovery::{
	self, RebindMode,
	tests::{self},
};

#[test]
fn rebind_state_allows_missing_marker_partial_in_progress_handoff() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Progress");
	let transition = recovery::validate_rebind_issue_state_for_policy(
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
	let transition = recovery::validate_rebind_issue_state_for_policy(
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
	let transition = recovery::validate_rebind_issue_state_for_policy(
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
	let transition = recovery::validate_rebind_issue_state_for_policy(
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
		let error = recovery::validate_rebind_issue_state_for_policy(
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
	let error = recovery::validate_rebind_issue_state_for_policy(
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
	let transition = recovery::validate_adopt_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&in_progress,
	)
	.expect("in-progress issue should be adoptable")
	.expect("in-progress issue should transition to review");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");

	let in_review = tests::sample_issue("In Review");
	let no_transition = recovery::validate_adopt_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&in_review,
	)
	.expect("in-review issue should remain adoptable");

	assert!(no_transition.is_none());

	let todo = tests::sample_issue("Todo");
	let error =
		recovery::validate_adopt_issue_state_for_policy(workflow.frontmatter().tracker(), &todo)
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

	let error = recovery::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover must not adopt pending checks");

	assert!(error.to_string().contains("still waiting on checks"));
}
