use crate::{
	execution_program::{ExecutionProgram, ExecutionProgramDependency, ExecutionQueueIntent},
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
};

#[test]
fn removed_flat_goal_contract_fields_migrate_before_direct_program_selection() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_field_issue =
		support::program_reconciler_issue("issue-removed-flat-contract", "PUB-209", "Todo", &[]);
	let ready_issue = support::program_reconciler_issue("issue-ready", "PUB-210", "Todo", &[]);
	let contract = support::program_reconciler_accepted_contract();
	let removed_field_program = ExecutionProgram::from_accepted_contract(
		"program-removed-flat-contract",
		config.service_id(),
		&contract,
		vec![support::program_reconciler_node(
			"node-removed-flat-contract",
			&removed_field_issue,
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("removed-field program should build");
	let current_program = ExecutionProgram::from_issue_batch_intake(
		"program-current-issue-batch",
		config.service_id(),
		"program-current-fingerprint",
		"Current issue-batch intake.",
		vec![support::program_reconciler_node(
			"node-ready",
			&ready_issue,
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("current program should build");

	store
		.upsert_execution_program(config.service_id(), removed_field_program)
		.expect("removed-field program should persist");

	support::insert_removed_flat_decision_contract(
		&state_path,
		config.service_id(),
		Some(&removed_field_issue.id),
		&contract,
	);

	store
		.upsert_execution_program(config.service_id(), current_program)
		.expect("current program should persist");

	drop(store);

	let store = StateStore::open(&state_path).expect("removed contract fields should migrate");
	let migrated_contract = store
		.decision_contract(config.service_id(), contract.contract_id())
		.expect("migrated contract should read")
		.expect("migrated contract should exist");

	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated before dispatch."
	);

	let tracker = FakeTracker::new(vec![removed_field_issue, ready_issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("removed flat fields should not abort program dispatch selection");
	let selected = selection.selected.expect("current issue-batch node should be selected");

	assert_eq!(selected.issue.id, ready_issue.id);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
	assert_eq!(selection.summary.programs_evaluated, 2);
	assert_eq!(selection.summary.dispatchable_nodes, 1);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn unlocks_downstream_node_when_dependency_reaches_terminal_state() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let dependency_todo =
		support::program_reconciler_issue("issue-dependency", "PUB-202", "Todo", &[]);
	let dependency_done =
		support::program_reconciler_issue("issue-dependency", "PUB-202", "Done", &[]);
	let dependent = support::program_reconciler_issue("issue-dependent", "PUB-203", "Todo", &[]);
	let dependent_node = support::program_reconciler_node(
		"node-dependent",
		&dependent,
		ExecutionQueueIntent::ReadyToQueue,
	)
	.with_dependencies([
		ExecutionProgramDependency::new("node-dependency").expect("dependency should build")
	])
	.expect("dependency should attach");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node(
					"node-dependency",
					&dependency_todo,
					ExecutionQueueIntent::Done,
				),
				dependent_node,
			]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![dependency_todo, dependent.clone()]);
	let blocked = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("blocked program dispatch selection should succeed");

	assert!(blocked.selected.is_none());
	assert_eq!(blocked.summary.dispatchable_nodes, 0);
	assert!(tracker.label_additions.borrow().is_empty());

	let tracker = FakeTracker::new(vec![dependency_done, dependent.clone()]);
	let ready = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("unlocked program dispatch selection should succeed");
	let selected = ready.selected.expect("dependent should be selected");

	assert_eq!(ready.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, dependent.id);
	assert!(tracker.label_additions.borrow().is_empty());
}
