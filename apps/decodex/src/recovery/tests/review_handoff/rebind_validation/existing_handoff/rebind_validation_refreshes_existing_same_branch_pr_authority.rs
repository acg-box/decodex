use crate::{
	recovery::{self, RebindMode, tests},
	state::{ReviewLifecycleHandoffFixture, ReviewLifecycleRecord},
};

#[test]
fn rebind_validation_refreshes_existing_same_branch_pr_authority() {
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue("In Review");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewLifecycleHandoffFixture::new(
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
		&ReviewLifecycleRecord::from_test_lifecycle_fixtures(&handoff, None),
		&landing_state,
		"1123456789abcdef0123456789abcdef01234567",
	)
	.expect("stale existing lifecycle authority should be refreshable");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, RebindMode::RefreshExistingHandoff);
}
