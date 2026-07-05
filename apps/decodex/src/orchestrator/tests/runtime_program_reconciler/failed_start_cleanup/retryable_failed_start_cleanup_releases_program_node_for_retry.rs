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
fn retryable_failed_start_cleanup_releases_program_node_for_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let ready_issue = support::program_reconciler_issue("issue-retry", "PUB-211", "Todo", &[]);
	let active_issue = support::program_reconciler_issue(
		"issue-retry",
		"PUB-211",
		"In Progress",
		&[active_label.as_str()],
	);
	let cleaned_issue = support::program_reconciler_issue("issue-retry", "PUB-211", "Todo", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&ready_issue.identifier, false)
		.expect("worktree should exist");
	let run_id = String::from("pub-211-attempt-1-123");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-retry",
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
		.record_run_attempt(&run_id, &ready_issue.id, 1, "failed")
		.expect("run attempt should record");

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![active_issue.clone()],
		vec![vec![active_issue.clone()], vec![cleaned_issue.clone()]],
	);
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
		attempt_number: 1,
		run_id: run_id.clone(),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected before thread start."),
		"thread/start",
		true,
	));

	orchestrator::handle_failure(&tracker, &config, &workflow, &store, &issue_run, &error)
		.expect("retryable failed-start cleanup should succeed");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(ready_issue.id.clone(), String::from("state-todo"))),
		"retryable failed-start cleanup should return the issue to the startable failure state"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[(ready_issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(
		store.worktree_for_issue(&ready_issue.id).expect("worktree lookup should work").is_none(),
		"no-diff failed-start cleanup should clear the retained worktree mapping"
	);
	assert!(
		store.lease_for_issue(&ready_issue.id).expect("lease lookup should work").is_none(),
		"cleanup should not leave a live lease"
	);
	assert!(
		store
			.list_private_execution_events(config.service_id(), &ready_issue.id, &run_id, 1)
			.expect("private events should list")
			.iter()
			.any(|event| event.event_type() == "retryable_failed_start_cleanup"),
		"cleanup should leave private audit evidence after active ownership is removed"
	);

	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("next program pass should evaluate");
	let selected =
		selection.selected.expect("cleaned failed-start node should be selectable again");

	assert_eq!(selection.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, ready_issue.id);
	assert_eq!(selected.dispatch_mode, IssueDispatchMode::Program);
}
