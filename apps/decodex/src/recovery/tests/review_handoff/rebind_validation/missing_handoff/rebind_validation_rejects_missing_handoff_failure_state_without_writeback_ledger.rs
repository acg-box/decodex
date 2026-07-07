use tempfile::TempDir;

use crate::recovery::{
	self, RebindMode, RecoveryRuntimeMutationPolicy,
	tests::{self},
};

#[test]
fn rebind_validation_rejects_missing_handoff_failure_state_without_writeback_ledger() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let issue = tests::sample_issue_with_labels("Todo", &[String::from("decodex:needs-attention")]);
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);

	context
		.state_store
		.record_run_attempt("pub-718-attempt-1", &issue.id, 1, "failed")
		.expect("failed attempt should record");

	let (_run_id, _attempt_number, mode) = recovery::validate_rebind_existing_handoff(
		&context,
		&issue,
		&worktree,
		None,
		&landing_state,
		head_oid,
	)
	.expect("missing handoff should load latest attempt");
	let error = recovery::validate_rebind_issue_state_for_policy(
		context.workflow.frontmatter().tracker(),
		&issue,
		mode,
	)
	.expect_err("failure state without writeback ledger should remain rejected");

	assert_eq!(mode, RebindMode::RestoreMissingHandoff);
	assert!(error.to_string().contains("review handoff rebind requires"));
}
