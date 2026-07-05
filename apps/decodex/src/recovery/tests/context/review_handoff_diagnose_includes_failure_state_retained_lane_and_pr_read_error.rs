use tempfile::TempDir;

use crate::{
	recovery::{
		self, RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::ReviewHandoffMarker,
};

#[test]
fn review_handoff_diagnose_includes_failure_state_retained_lane_and_pr_read_error() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let issue = tests::sample_issue_with_labels("Todo", &[String::from("decodex:needs-attention")]);
	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let branch_name = "x/pubfi-pub-718";
	let (worktree_dir, _original_head, head_oid) = tests::temp_git_worktree(branch_name);

	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			branch_name,
			&worktree_dir.path().display().to_string(),
		)
		.expect("retained worktree should record");
	context
		.state_store
		.record_run_attempt("pub-718-attempt-1", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	context
		.state_store
		.upsert_review_handoff_marker(
			context.config.service_id(),
			&issue.id,
			&ReviewHandoffMarker::new(
				"pub-718-attempt-1",
				1,
				branch_name,
				pr_url,
				"main",
				branch_name,
				&head_oid,
			),
		)
		.expect("review handoff marker should record");

	let diagnostics =
		recovery::diagnose_all_retained_review_worktrees_with_tracker(&context, &tracker)
			.expect("retained review diagnostics should build");
	let diagnostic = diagnostics.first().expect("retained failure-state diagnostic should render");

	assert_eq!(diagnostics.len(), 1);
	assert_eq!(diagnostic.issue_identifier, "PUB-718");
	assert_eq!(diagnostic.issue_state, "Todo");
	assert_eq!(diagnostic.reason, "pull_request_state_read_failed");
	assert_eq!(diagnostic.existing_pr_url.as_deref(), Some(pr_url));
	assert_eq!(diagnostic.local_head_oid.as_deref(), Some(head_oid.as_str()));
	assert_eq!(diagnostic.active_label_present, Some(false));
	assert!(
		diagnostic.pr_read_error.as_deref().is_some_and(|error| !error.trim().is_empty()),
		"diagnose should report the concrete external PR read failure"
	);
}
