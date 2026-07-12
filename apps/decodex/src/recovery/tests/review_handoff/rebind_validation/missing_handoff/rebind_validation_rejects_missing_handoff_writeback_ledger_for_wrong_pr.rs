use tempfile::TempDir;

use crate::recovery::{
	self, RebindMode, RecoveryRuntimeMutationPolicy,
	tests::{
		review_handoff::rebind_validation::support,
		{self},
	},
};

#[test]
fn rebind_validation_rejects_missing_handoff_writeback_ledger_for_wrong_pr() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let issue = tests::sample_issue_with_labels("Todo", &[String::from("decodex:needs-attention")]);
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let mut event = support::terminal_writeback_failure_event(
		context.config.service_id(),
		&issue,
		"pub-718-attempt-1",
		1,
		branch_name,
		"https://github.com/hack-ink/pubfi-mono-v2/pull/99",
	);

	event.pr_head_sha = Some(head_oid.to_owned());

	context
		.state_store
		.record_lane_run_attempt(
			context.config.service_id(),
			"pub-718-attempt-1",
			&issue.id,
			1,
			"failed",
		)
		.expect("failed attempt should record");
	context
		.state_store
		.record_linear_execution_event(&event)
		.expect("terminal ledger event should record");

	let (_run_id, _attempt_number, mode) = recovery::validate_rebind_existing_handoff(
		&context,
		&issue,
		&worktree,
		None,
		&landing_state,
		head_oid,
	)
	.expect("missing handoff should load latest attempt");

	assert_eq!(mode, RebindMode::RestoreMissingHandoff);
}
