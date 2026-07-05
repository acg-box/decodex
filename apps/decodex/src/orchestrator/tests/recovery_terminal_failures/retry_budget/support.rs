use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, ServiceConfig, WorkflowDocument,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeSpec,
};

pub(in crate::orchestrator::tests) fn assert_retryable_failure_writeback_does_not_require_attention(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	case_number: usize,
	error: Report,
	expected_error_class: &str,
) {
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = format!("issue-{case_number}");
	let issue_identifier = format!("PUB-10{case_number}");
	let issue = tests::sample_issue_with_sort_fields(
		&issue_id,
		&issue_identifier,
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: format!("x/pubfi-{}", issue_identifier.to_lowercase()),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join(&issue.identifier),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: format!("pub-10{case_number}-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, config, workflow, &state_store, &issue_run, &error)
		.expect("retryable failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains(expected_error_class)
	}));
	assert!(comments.iter().all(|comment| {
		!comment.contains("decodex run failed and needs attention")
			&& !comment.contains("decodex retained partial progress and needs attention")
	}));
	assert!(
		comments
			.iter()
			.all(|comment| { records::parse_linear_execution_event_record(comment).is_none() })
	);
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}

pub(in crate::orchestrator::tests) fn assert_dirty_retryable_failure_writeback_does_not_require_attention(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	case_number: usize,
	error: Report,
	expected_error_class: &str,
) {
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = format!("issue-dirty-{case_number}");
	let issue_identifier = format!("PUB-30{case_number}");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue_with_sort_fields(
		&issue_id,
		&issue_identifier,
		"In Progress",
		&[active_label.as_str()],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let branch_name = format!("x/pubfi-{}", issue_identifier.to_lowercase());
	let worktree_rel_path = format!(".worktrees/{issue_identifier}");
	let worktree_path = config.worktree_root().join(&issue_identifier);

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", &branch_name, &worktree_rel_path, "main"],
	);
	fs::write(
		worktree_path.join("README.md"),
		format!("dirty retryable recovery case {case_number}\n"),
	)
	.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name,
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
		run_id: format!("pub-30{case_number}-attempt-1-123"),
		retry_budget_base: 0,
	};

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, config, workflow, &state_store, &issue_run, &error)
		.expect("dirty retryable failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains(expected_error_class)
	}));
	assert!(
		comments.iter().all(|comment| {
			!comment.contains("decodex retained partial progress and needs attention")
				&& !comment.contains("decodex run failed and needs attention")
		}),
		"retained tracked changes must not force manual attention for `{expected_error_class}` while retry budget remains"
	);
	assert!(
		comments
			.iter()
			.all(|comment| { records::parse_linear_execution_event_record(comment).is_none() })
	);
	assert!(
		state_store
			.list_linear_execution_events(config.service_id(), &issue.id)
			.expect("linear execution events should list")
			.is_empty()
	);
}
