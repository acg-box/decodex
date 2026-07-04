use tempfile::TempDir;

use crate::{
	recovery::{
		RebindMode, RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
	tracker::{
		TrackerIssue, TrackerLabel,
		records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
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
	let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
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
	let (_run_id, _attempt_number, mode) = super::validate_existing_handoff_refresh(
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

	let error = super::validate_rebind_issue_state_for_policy(
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
	let error = super::validate_existing_handoff_refresh(
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
	let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
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
	let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
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
		.record_linear_execution_event(&terminal_writeback_failure_event(
			context.config.service_id(),
			&issue,
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
		))
		.expect("terminal ledger event should record");

	let (run_id, attempt_number, mode) = super::validate_rebind_existing_handoff(
		&context,
		&issue,
		&worktree,
		None,
		None,
		&landing_state,
		head_oid,
	)
	.expect("writeback-failure ledger should allow missing handoff recovery mode");
	let transition = super::validate_rebind_issue_state_for_policy(
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

	let (_run_id, _attempt_number, mode) = super::validate_rebind_existing_handoff(
		&context,
		&issue,
		&worktree,
		None,
		None,
		&landing_state,
		head_oid,
	)
	.expect("missing handoff should load latest attempt");
	let error = super::validate_rebind_issue_state_for_policy(
		context.workflow.frontmatter().tracker(),
		&issue,
		mode,
	)
	.expect_err("failure state without writeback ledger should remain rejected");

	assert_eq!(mode, RebindMode::RestoreMissingHandoff);
	assert!(error.to_string().contains("review handoff rebind requires"));
}

#[test]
fn rebind_label_validation_restores_active_and_clears_attention_for_missing_writeback_failure() {
	let workflow = tests::sample_workflow();
	let mut issue =
		tests::sample_issue_with_labels("Todo", &[String::from("decodex:needs-attention")]);

	issue.team.labels.push(TrackerLabel {
		id: String::from("label-decodex-active-pubfi"),
		name: String::from("decodex:active:pubfi"),
	});

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let labels = super::validate_rebind_tracker_labels_with_tracker(
		&tracker,
		"pubfi",
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::RestoreMissingHandoffAfterWritebackFailure,
	)
	.expect("writeback-failure missing handoff should restore ownership and clear attention");

	assert!(!labels.active_label_present);
	assert!(labels.restore_active_label);
	assert!(labels.clear_needs_attention_label);
}

fn terminal_writeback_failure_event(
	service_id: &str,
	issue: &TrackerIssue,
	run_id: &str,
	attempt_number: i64,
	branch_name: &str,
	pr_url: &str,
) -> LinearExecutionEventRecord {
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id,
			attempt_number,
		},
		"terminal_failure",
		String::from("2026-07-04T00:00:00Z"),
		"review-handoff-writeback-failed",
	);

	event.branch = Some(branch_name.to_owned());
	event.worktree_path = Some(format!(".worktrees/{}", issue.identifier));
	event.pr_url = Some(pr_url.to_owned());
	event.error_class = Some(String::from("review_handoff_writeback_failed"));
	event.next_action = Some(String::from("recover review handoff"));
	event.blockers = Some(vec![String::from("review handoff writeback failed")]);
	event.evidence = Some(vec![String::from("retained PR lane evidence present")]);

	event
}
