use tempfile::TempDir;

use crate::recovery::{
	self, RebindMode, RecoveryRuntimeMutationPolicy,
	tests::{
		review_handoff::rebind_validation::support,
		{self},
	},
};

#[test]
fn rebind_validation_allows_missing_handoff_after_writeback_failure_ledger() {
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
	context
		.state_store
		.record_linear_execution_event(&support::terminal_writeback_failure_event(
			context.config.service_id(),
			&issue,
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
		))
		.expect("terminal ledger event should record");

	let (run_id, attempt_number, mode) = recovery::validate_rebind_existing_handoff(
		&context,
		&issue,
		&worktree,
		None,
		&landing_state,
		head_oid,
	)
	.expect("writeback-failure ledger should allow missing handoff recovery mode");
	let transition = recovery::validate_rebind_issue_state_for_policy(
		context.workflow.frontmatter().tracker(),
		&issue,
		mode,
	)
	.expect("writeback-failure missing handoff should allow failure-state recovery")
	.expect("failure-state recovery should transition to review");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, RebindMode::RestoreMissingHandoffAfterWritebackFailure);
	assert_eq!(transition.state_name, "In Review");
}
