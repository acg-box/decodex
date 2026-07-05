use color_eyre::Report;

use crate::{
	agent::AppServerTransportFailure,
	execution_program::ExecutionQueueIntent,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn retryable_failed_start_cleanup_preserves_open_handoff_phase() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let ready_issue = support::program_reconciler_issue("issue-handoff", "PUB-212", "Todo", &[]);
	let active_issue = support::program_reconciler_issue(
		"issue-handoff",
		"PUB-212",
		"In Progress",
		&[active_label.as_str()],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&ready_issue.identifier, false)
		.expect("worktree should exist");
	let current_run_id = String::from("pub-212-attempt-2-456");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-handoff",
				&ready_issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");
	store
		.upsert_worktree(
			config.service_id(),
			&ready_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_private_execution_event(
			config.service_id(),
			&ready_issue.id,
			"pub-212-attempt-1-123",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("open handoff phase should record");
	store
		.record_run_attempt(&current_run_id, &ready_issue.id, 2, "failed")
		.expect("current run attempt should record");

	let tracker = FakeTracker::new(vec![active_issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: active_issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: active_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Program,
		attempt_number: 2,
		run_id: current_run_id.clone(),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected before thread start."),
		"thread/start",
		true,
	));

	orchestrator::handle_failure(&tracker, &config, &workflow, &store, &issue_run, &error)
		.expect("retryable failed-start writeback should preserve open handoff ownership");

	assert!(
		tracker.state_updates.borrow().is_empty(),
		"open handoff phases must keep the issue in active ownership"
	);
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"open handoff phases must not clear the active label"
	);
	assert!(
		store.worktree_for_issue(&ready_issue.id).expect("worktree lookup should work").is_some(),
		"open handoff phases must retain the worktree mapping"
	);
	assert!(
		store
			.list_private_execution_events(config.service_id(), &ready_issue.id, &current_run_id, 2)
			.expect("private events should list")
			.iter()
			.all(|event| event.event_type() != "retryable_failed_start_cleanup"),
		"open handoff phases must not be audited as cleaned failed-start ownership"
	);
}
