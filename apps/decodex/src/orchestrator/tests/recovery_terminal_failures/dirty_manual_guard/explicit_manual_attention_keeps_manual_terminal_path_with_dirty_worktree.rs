use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, ManualAttentionRequested,
		tests::{
			FakeTracker, TEST_SERVICE_ID, {self},
		},
	},
	state::StateStore,
	tracker::{self, records},
	worktree::WorktreeSpec,
};

#[test]
fn explicit_manual_attention_keeps_manual_terminal_path_with_dirty_worktree() {
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
	fs::write(worktree_path.join("README.md"), "manual attention work\n")
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
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let error = Report::new(ManualAttentionRequested {
		issue_identifier: issue.identifier.clone(),
		label: String::from("decodex:needs-attention"),
		run_id: issue_run.run_id.clone(),
		error_class: None,
	});

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("manual attention should keep its terminal path");

	let ledger_event = tracker
		.comments
		.borrow()
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("manual attention should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("human_attention_required"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("manual_attention"));
	assert_eq!(ledger_event.summary.as_deref(), Some("Decodex run failed and needs attention."));
}
