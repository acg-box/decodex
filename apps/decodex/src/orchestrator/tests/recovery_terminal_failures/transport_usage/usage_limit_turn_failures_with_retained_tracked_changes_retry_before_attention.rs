use std::fs;

use color_eyre::Report;

use crate::{
	agent::AppServerTurnFailure,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self},
	worktree::WorktreeSpec,
};

#[test]
fn usage_limit_turn_failures_with_retained_tracked_changes_retry_before_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-103");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-103", ".worktrees/PUB-103", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained usage-limit patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-103"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-103-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTurnFailure::new(
		"thread-1",
		Some(String::from("turn-1")),
		"failed",
		"You've hit your usage limit.",
		Some(String::from("usageLimitExceeded")),
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty usage-limit failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_usage_limit_exceeded")
			&& comment.contains("reselect or refresh the Codex account")
	}));
	assert!(
		tracker.comments.borrow().iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention while usage-limit retry remains"
	);
}
