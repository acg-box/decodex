use crate::{
	recovery::{self, RebindMode, tests},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
};

#[test]
fn rebind_validation_refreshes_existing_same_branch_pr_marker() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Review");
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
	let (run_id, attempt_number, mode) = recovery::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		None,
		&landing_state,
		"1123456789abcdef0123456789abcdef01234567",
	)
	.expect("stale existing marker should be refreshable");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, RebindMode::RefreshExistingHandoff);
}

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
		&handoff,
		None,
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

#[test]
fn rebind_validation_rejects_current_existing_marker_as_noop() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Review");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let error = recovery::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		Some(&orchestration),
		&landing_state,
		head_oid,
	)
	.expect_err("current existing marker should not be rebound");

	assert!(error.to_string().contains("no rebind is needed"));
}

#[test]
fn rebind_validation_completes_current_existing_marker_state_transition() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Progress");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let (run_id, attempt_number, mode) = recovery::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		Some(&orchestration),
		&landing_state,
		head_oid,
	)
	.expect("current marker should allow state-only handoff completion");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, RebindMode::CompleteExistingHandoffState);
}

#[test]
fn rebind_validation_completes_current_existing_marker_failure_state_drift() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("Todo");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let (run_id, attempt_number, mode) = recovery::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		Some(&orchestration),
		&landing_state,
		head_oid,
	)
	.expect("current marker should allow failure-state drift completion");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, RebindMode::CompleteExistingHandoffState);
}
