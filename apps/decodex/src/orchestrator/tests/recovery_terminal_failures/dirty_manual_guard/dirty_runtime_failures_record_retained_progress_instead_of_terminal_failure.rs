use std::fs;

use color_eyre::Report;

use crate::{
	agent::AppServerCapabilityPreflightFailure,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeSpec,
};

#[test]
fn dirty_runtime_failures_record_retained_progress_instead_of_terminal_failure() {
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
	fs::write(worktree_path.join("README.md"), "retained runtime recovery work\n")
		.expect("tracked worktree file should change");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
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
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 1,
	};
	let error = Report::new(AppServerCapabilityPreflightFailure::blocked_for_test(
		"model",
		"configured model was not present in model/list.",
	));

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("dirty runtime failure should retain partial progress");

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("app_server_runtime_preflight_failed")
	}));
	assert!(
		comments.iter().all(|comment| !comment.contains("decodex run failed and needs attention"))
	);

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained runtime failure should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert!(
		ledger_event.evidence.as_deref().is_some_and(|evidence| evidence
			.iter()
			.any(|item| item.contains("app_server_runtime_preflight_failed"))),
		"retained progress evidence should preserve the source runtime error class"
	);
}
