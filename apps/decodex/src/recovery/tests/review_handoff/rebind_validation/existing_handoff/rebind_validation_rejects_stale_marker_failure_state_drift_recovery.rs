use crate::{
	recovery::{self, RebindMode, tests},
	state::{ReviewHandoffMarker, ReviewLifecycleRecord},
};

#[test]
fn rebind_validation_rejects_stale_marker_failure_state_drift_recovery() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("Todo");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		"0123456789abcdef0123456789abcdef01234567",
	);
	let landing_state = tests::sample_landing_state(
		pr_url,
		branch_name,
		"1123456789abcdef0123456789abcdef01234567",
	);
	let (_run_id, _attempt_number, mode) = recovery::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&ReviewLifecycleRecord::from_test_review_markers(&handoff, None),
		&landing_state,
		"1123456789abcdef0123456789abcdef01234567",
	)
	.expect("stale existing marker should require marker refresh first");

	assert_eq!(mode, RebindMode::RefreshExistingHandoff);

	let error = recovery::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		mode,
	)
	.expect_err("stale marker refresh must not repair failure-state drift");

	assert!(error.to_string().contains("review handoff rebind requires"));
}
