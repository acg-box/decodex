use crate::{
	recovery::{self, tests},
	state::{ReviewHandoffMarker, ReviewLifecycleRecord, ReviewOrchestrationMarker},
};

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
		&ReviewLifecycleRecord::from_test_review_markers(&handoff, Some(&orchestration)),
		&landing_state,
		head_oid,
	)
	.expect_err("current existing marker should not be rebound");

	assert!(error.to_string().contains("no rebind is needed"));
}
