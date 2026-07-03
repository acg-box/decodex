use std::fs;

use color_eyre::Report;

use crate::{
	agent::AppServerPhaseGoalFailure,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, PhaseGoalKind,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::{self, StateStore},
	tracker::{self},
	worktree::WorktreeSpec,
};

#[test]
fn phase_goal_terminal_path_missing_retries_before_attention_budget_is_exhausted() {
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
	let error = Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
		PhaseGoalKind::HandoffEvidence,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("missing terminal path should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("phase_goal_terminal_path_missing")
			&& comment.contains("terminal-path recovery automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);

	let marker = state::read_run_activity_marker_snapshot(&issue_run.worktree.path)
		.expect("retry schedule should be readable")
		.expect("retry schedule marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
}

#[test]
fn phase_goal_terminal_path_missing_with_retained_changes_retries_before_attention() {
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
	fs::write(worktree_path.join("README.md"), "retained handoff patch\n")
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
	let error = Report::new(AppServerPhaseGoalFailure::missing_terminal_path_for_test(
		PhaseGoalKind::HandoffEvidence,
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty terminal-path failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("phase_goal_terminal_path_missing")
	}));
	assert!(
		tracker.comments.borrow().iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention during terminal-path retry"
	);
}
