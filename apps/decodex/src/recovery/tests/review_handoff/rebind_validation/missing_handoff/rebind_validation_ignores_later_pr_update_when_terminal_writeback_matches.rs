use tempfile::TempDir;

use crate::{
	recovery::{
		self, RebindMode, RecoveryRuntimeMutationPolicy,
		tests::{
			review_handoff::rebind_validation::support,
			{self},
		},
	},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

#[test]
fn rebind_validation_ignores_later_pr_update_when_terminal_writeback_matches() {
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
		.record_linear_execution_event(&support::terminal_writeback_failure_event(
			context.config.service_id(),
			&issue,
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
		))
		.expect("terminal ledger event should record");

	let mut pr_update = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "pub-718-attempt-1",
			attempt_number: 1,
		},
		"pr_updated",
		String::from("2026-07-04T00:01:00Z"),
		"post-terminal-pr-update",
	);

	pr_update.branch = Some(branch_name.to_owned());
	pr_update.commit_sha = Some(head_oid.to_owned());
	pr_update.pr_url = Some(pr_url.to_owned());
	pr_update.pr_head_sha = Some(head_oid.to_owned());
	pr_update.pr_base_ref = Some(String::from("main"));

	context
		.state_store
		.record_linear_execution_event(&pr_update)
		.expect("post-terminal pr event should record");

	let (_run_id, _attempt_number, mode) = recovery::validate_rebind_existing_handoff(
		&context,
		&issue,
		&worktree,
		None,
		&landing_state,
		head_oid,
	)
	.expect("terminal writeback event should remain authoritative");

	assert_eq!(mode, RebindMode::RestoreMissingHandoffAfterWritebackFailure);
}
