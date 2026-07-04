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

#[test]
fn rebind_validation_rejects_missing_handoff_writeback_ledger_for_wrong_branch() {
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
		"x/pubfi-other",
		pr_url,
	);

	event.pr_head_sha = Some(head_oid.to_owned());

	context
		.state_store
		.record_run_attempt("pub-718-attempt-1", &issue.id, 1, "failed")
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
		None,
		&landing_state,
		head_oid,
	)
	.expect("missing handoff should load latest attempt");

	assert_eq!(mode, RebindMode::RestoreMissingHandoff);
}

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
		.record_run_attempt("pub-718-attempt-1", &issue.id, 1, "failed")
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
		None,
		&landing_state,
		head_oid,
	)
	.expect("missing handoff should load latest attempt");

	assert_eq!(mode, RebindMode::RestoreMissingHandoff);
}

#[test]
fn rebind_validation_rejects_missing_handoff_writeback_ledger_for_wrong_head() {
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
		pr_url,
	);

	event.pr_head_sha = Some(String::from("2123456789abcdef0123456789abcdef01234567"));

	context
		.state_store
		.record_run_attempt("pub-718-attempt-1", &issue.id, 1, "failed")
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
		None,
		&landing_state,
		head_oid,
	)
	.expect("missing handoff should load latest attempt");

	assert_eq!(mode, RebindMode::RestoreMissingHandoff);
}

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
		None,
		&landing_state,
		head_oid,
	)
	.expect("terminal writeback event should remain authoritative");

	assert_eq!(mode, RebindMode::RestoreMissingHandoffAfterWritebackFailure);
}

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
