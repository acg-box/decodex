use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, RepoGateFailure, RepoGateFailureKind,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeSpec,
};

#[test]
fn terminal_failure_with_retained_tracked_changes_records_retained_partial_progress() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-101", ".worktrees/PUB-101", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained terminal patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
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
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::CommandSpawnFailed,
		String::from("Failed to spawn repo gate command `cargo make test`."),
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("terminal failure handling should succeed");

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("finish validation and PR handoff or reset the patch manually")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and needs attention"))
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained partial progress should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("Source failure class `repo_gate_command_spawn_failed`"))),
		"retained partial progress evidence should preserve the source failure class"
	);
}

#[test]
fn repo_gate_tracked_rewrites_left_records_retained_partial_progress_without_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-102");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-102", ".worktrees/PUB-102", "main"],
	);
	fs::write(worktree_path.join("README.md"), "repo gate left tracked rewrites\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-102"),
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
		run_id: String::from("pub-102-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::TrackedRewritesLeft,
		String::from("Repo gate verification left tracked-file rewrites."),
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("tracked repo-gate rewrites should retain partial progress");

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("finish validation and PR handoff or reset the patch manually")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and will retry")),
		"tracked repo-gate rewrites should not continue automatic retry"
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained partial progress should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("Source failure class `repo_gate_tracked_rewrites_left`"))),
		"retained progress evidence should preserve the source repo-gate failure class"
	);
}

#[test]
fn retryable_runtime_failure_with_retained_tracked_changes_retries_before_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-102");

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-102", ".worktrees/PUB-102", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained validation-ready runtime patch\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-102"),
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
		run_id: String::from("pub-102-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::msg(
		"app-server run ended after a validation-ready checkpoint without a terminal path",
	);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty generic runtime failure should remain retryable");

	let comments = tracker.comments.borrow();

	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex run failed and will retry")
			&& comment.contains("retryable_execution_failure")
			&& comment.contains("decodex will retry automatically")
	}));
	assert!(
		comments
			.iter()
			.all(|comment| !comment
				.contains("decodex retained partial progress and needs attention")),
		"retained work must not force manual attention while retry budget remains"
	);
	assert!(
		comments
			.iter()
			.all(|comment| records::parse_linear_execution_event_record(comment).is_none()),
		"retryable retained work should not write a terminal needs-attention ledger event"
	);
}
