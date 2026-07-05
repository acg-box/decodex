use crate::{
	execution_program::ExecutionQueueIntent,
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
};

#[test]
fn needs_attention_node_is_not_selected_even_with_stale_queue_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = support::program_reconciler_queue_label();
	let issue = support::program_reconciler_issue(
		"issue-attention",
		"PUB-206",
		"Todo",
		&[queue_label.as_str(), "decodex:needs-attention"],
	);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node_with_mapping(
					"node-attention",
					&issue,
					ExecutionQueueIntent::ReadyToQueue,
				),
			]),
		)
		.expect("program should persist");
	store
		.record_run_attempt("pub-206-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_lease(config.service_id(), &issue.id, "pub-206-attempt-1", "In Progress")
		.expect("lease should record");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("attention program dispatch selection should succeed");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());

	let snapshot =
		orchestrator::build_live_operator_status_snapshot(&tracker, &config, &workflow, &store, 10)
			.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should render");
	let node = program.node_readbacks.first().expect("attention node should render");

	assert_eq!(node.lifecycle_state, "needs_attention");
	assert!(node.reason_codes.contains(&String::from("mapped_issue_needs_attention")));
}
