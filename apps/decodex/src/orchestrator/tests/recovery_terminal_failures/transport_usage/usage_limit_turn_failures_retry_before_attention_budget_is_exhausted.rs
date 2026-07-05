use std::fs;

use color_eyre::Report;

use crate::{
	agent::AppServerTurnFailure,
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
fn usage_limit_turn_failures_retry_before_attention_budget_is_exhausted() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
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
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"You've hit your usage limit.",
		Some(String::from("usageLimitExceeded")),
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("usage limit failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_usage_limit_exceeded")
			&& comment.contains("reselect or refresh the Codex account")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);
}
