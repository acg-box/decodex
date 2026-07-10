use crate::{
	execution_program::ExecutionQueueIntent,
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
	tracker,
};

#[test]
fn stale_issue_batch_active_intent_releases_after_ownership_clears() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let active_issue = support::program_reconciler_issue(
		"issue-stale-active",
		"PUB-216",
		"Todo",
		&[active_label.as_str()],
	);
	let cleaned_issue =
		support::program_reconciler_issue("issue-stale-active", "PUB-216", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![support::program_reconciler_node(
				"node-stale-active",
				&active_issue,
				ExecutionQueueIntent::Active,
			)]),
		)
		.expect("active issue-batch program should persist");

	let tracker = FakeTracker::new(vec![cleaned_issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("cleaned issue-batch program should reconcile");
	let selected = selection.selected.expect("cleaned node should become dispatchable");
	let program = store
		.list_execution_programs(config.service_id())
		.expect("program list should read")
		.pop()
		.expect("program should remain");

	assert_eq!(selected.issue.id, cleaned_issue.id);
	assert_eq!(selection.summary.programs_updated, 1);
	assert_eq!(program.program().nodes()[0].queue_intent(), ExecutionQueueIntent::ReadyToQueue);
}
