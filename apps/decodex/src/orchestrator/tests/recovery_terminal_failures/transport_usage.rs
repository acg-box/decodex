use std::fs;

use color_eyre::Report;

use crate::{
	agent::{AppServerTransportFailure, AppServerTurnFailure},
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
fn startup_transport_failures_retry_before_attention_budget_is_exhausted() {
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
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected unexpectedly."),
		"thread/start",
		true,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("startup transport failure should remain retryable");

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("app_server_transport_disconnected")
			&& comment.contains("thread/start")
			&& comment.contains("restart the app-server and retry automatically")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention"))
	);
}

#[test]
fn exhausted_startup_transport_retry_budget_requires_attention_with_transport_class() {
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
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected unexpectedly."),
		"thread/start",
		true,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted startup transport failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_transport_disconnected")
			&& comment.contains("failure during `thread/start` manually")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}

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

#[test]
fn exhausted_usage_limit_retry_budget_requires_attention_with_usage_class() {
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
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
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
		.expect("exhausted usage limit failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_usage_limit_exceeded")
			&& comment.contains("inspect Codex account usage")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}
