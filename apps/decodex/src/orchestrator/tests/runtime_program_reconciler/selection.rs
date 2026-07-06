use std::time::Instant;

use crate::{
	execution_program::ExecutionQueueIntent,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, RetryDispatchDecision, RetryEntry,
		RetryEntryLifecycle, RetryKind, RetryQueue,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn selects_ready_node_for_direct_program_dispatch_without_queue_label_mutation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue("issue-ready", "PUB-201", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-ready",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("ready program dispatch selection should succeed");
	let selected = selection.selected.expect("ready node should be selected");

	assert_eq!(selection.summary.programs_evaluated, 1);
	assert_eq!(selection.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, issue.id);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);

	let program_dispatch =
		selected.program_dispatch.clone().expect("selection should preserve program dispatch");

	assert_eq!(program_dispatch.program_id, "program-reconciler");
	assert_eq!(program_dispatch.node_id, "node-ready");
	assert_eq!(program_dispatch.source_contract_id, None);
	assert_eq!(program_dispatch.queue_intent, "ready_to_queue");
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn daemon_planning_preserves_program_dispatch_provenance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue("issue-daemon-ready", "PUB-204", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-daemon-ready",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let mut retry_queue = RetryQueue::default();
	let (summary, from_retry_queue) =
		orchestrator::plan_next_daemon_run(&mut retry_queue, &tracker, &config, &workflow, &store)
			.expect("daemon planning should succeed")
			.expect("daemon planning should select ready program node");
	let program_dispatch = summary.program_dispatch.expect("daemon summary should carry program");

	assert!(!from_retry_queue);
	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Program);
	assert_eq!(program_dispatch.program_id, "program-reconciler");
	assert_eq!(program_dispatch.node_id, "node-daemon-ready");
	assert_eq!(program_dispatch.queue_intent, "ready_to_queue");
}

#[test]
fn retained_nonterminal_worktree_blocks_no_conflict_program_dispatch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue =
		support::program_reconciler_issue("issue-retained-no-conflict", "PUB-205", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-retained-no-conflict",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");
	store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-205",
			&config.repo_root().display().to_string(),
		)
		.expect("retained worktree should persist");

	let tracker = FakeTracker::new(vec![issue]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("program dispatch selection should evaluate");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.programs_evaluated, 1);
	assert_eq!(selection.summary.dispatchable_nodes, 0);
}

#[test]
fn due_program_retry_preserves_program_dispatch_provenance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue =
		support::program_reconciler_issue("issue-due-program-retry", "PUB-206", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-due-program-retry",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Program,
		kind: RetryKind::Failure,
		attempt: 1,
		ready_at: Instant::now(),
	});

	let decision =
		orchestrator::plan_due_retry_run(&mut retry_queue, &tracker, &config, &workflow, &store)
			.expect("program retry planning should succeed");
	let RetryDispatchDecision::Dispatch(summary) = decision else {
		panic!("due program retry should dispatch");
	};
	let program_dispatch =
		summary.program_dispatch.expect("due program retry should carry Program selection");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Program);
	assert_eq!(program_dispatch.program_id, "program-reconciler");
	assert_eq!(program_dispatch.node_id, "node-due-program-retry");
	assert_eq!(program_dispatch.queue_intent, "ready_to_queue");
}

#[test]
fn skips_ready_node_without_dispatch_action() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue(
		"issue-manual-only",
		"PUB-203",
		"Todo",
		&["decodex:manual-only"],
	);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-manual-only",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("program dispatch selection should evaluate");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.programs_evaluated, 1);
	assert_eq!(selection.summary.dispatchable_nodes, 0);
}

#[test]
fn records_private_program_dispatch_selection_event_with_node_provenance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue("issue-ready-event", "PUB-202", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-ready-event",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("ready program dispatch selection should succeed");
	let selected = selection.selected.expect("ready node should be selected");
	let program_dispatch = selected.program_dispatch.expect("selection should carry provenance");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let issue_run = IssueRunPlan {
		issue: selected.issue,
		issue_state: String::from("Todo"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Program,
		attempt_number: 1,
		run_id: String::from("pub-202-attempt-1-123"),
		retry_budget_base: 0,
	};
	let event = orchestrator::record_program_dispatch_selected(
		&store,
		config.service_id(),
		&issue_run,
		&program_dispatch,
	)
	.expect("program dispatch selection event should record");

	assert_eq!(event.event_type(), "program_dispatch_selected");
	assert_eq!(event.payload()["schema"], "decodex.program_dispatch_selected/1");
	assert_eq!(event.payload()["issue"]["identifier"], "PUB-202");
	assert_eq!(event.payload()["run"]["dispatch_mode"], "program");
	assert_eq!(event.payload()["execution_program"]["program_id"], "program-reconciler");
	assert_eq!(event.payload()["execution_program"]["node_id"], "node-ready-event");
	assert_eq!(event.payload()["execution_program"]["queue_intent"], "ready_to_queue");

	let events = store
		.list_private_execution_events(config.service_id(), &issue.id, "pub-202-attempt-1-123", 1)
		.expect("private event should be readable");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].payload(), event.payload());

	let duplicate = orchestrator::record_program_dispatch_selected(
		&store,
		config.service_id(),
		&issue_run,
		&program_dispatch,
	)
	.expect("duplicate program dispatch selection should be idempotent");
	let events = store
		.list_private_execution_events(config.service_id(), &issue.id, "pub-202-attempt-1-123", 1)
		.expect("private event should be readable");

	assert_eq!(events.len(), 1);
	assert_eq!(duplicate.record_id(), event.record_id());

	let summary = orchestrator::run_summary_from_issue_run(config.service_id(), &issue_run);
	let summary_duplicate = orchestrator::record_program_dispatch_selected_for_summary(
		&store,
		&summary,
		&program_dispatch,
	)
	.expect("summary-based program dispatch selection should be idempotent");
	let events = store
		.list_private_execution_events(config.service_id(), &issue.id, "pub-202-attempt-1-123", 1)
		.expect("private event should be readable");

	assert_eq!(events.len(), 1);
	assert_eq!(summary_duplicate.record_id(), event.record_id());
}
