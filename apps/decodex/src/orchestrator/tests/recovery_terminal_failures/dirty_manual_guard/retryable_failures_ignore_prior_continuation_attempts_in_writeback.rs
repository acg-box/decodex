use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{
			FakeTracker, {self},
		},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn retryable_failures_ignore_prior_continuation_attempts_in_writeback() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 4,
		run_id: String::from("pub-101-attempt-4-123"),
		retry_budget_base: 0,
	};

	state_store
		.record_run_attempt("pub-101-attempt-1-123", &issue.id, 1, "succeeded")
		.expect("first continuation attempt should record");
	state_store
		.record_run_attempt("pub-101-attempt-2-123", &issue.id, 2, "succeeded")
		.expect("second continuation attempt should record");
	state_store
		.record_run_attempt("pub-101-attempt-3-123", &issue.id, 3, "succeeded")
		.expect("third continuation attempt should record");
	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("current failed attempt should record");

	orchestrator::handle_failure(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("command failed"),
	)
	.expect("retryable failure handling should succeed");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("retryable_execution_failure")
			&& comment.contains("- run_sequence_attempt: `4` (not retry-budget count)")
			&& comment.contains("- retry_budget_attempt: `1` / `3`")
	}));
	assert!(!tracker.comments.borrow().iter().any(|comment| {
		comment.contains("needs attention") || comment.contains("retry_budget_exhausted")
	}));
}
