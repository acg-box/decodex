use crate::{
	execution_program::ExecutionQueueIntent,
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
};

#[test]
fn excluded_ready_issue_is_not_selected() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue("issue-excluded", "PUB-207", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node_with_mapping(
					"node-excluded",
					&issue,
					ExecutionQueueIntent::ReadyToQueue,
				),
			]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[issue.id.as_str()],
	)
	.expect("excluded program dispatch selection should succeed");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);
	assert!(tracker.label_removals.borrow().is_empty());
}
