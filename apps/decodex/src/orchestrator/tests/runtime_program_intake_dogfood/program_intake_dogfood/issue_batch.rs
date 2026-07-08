use crate::{
	config::ServiceConfig,
	orchestrator,
	orchestrator::tests::runtime_program_intake_dogfood::program_intake_dogfood::support::{
		self, DogfoodTracker,
	},
	program_intake::{self},
	state::StateStore,
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

#[test]
fn issue_batch_apply_direct_dispatch_is_end_to_end() {
	let (_temp_dir, config, workflow) = super::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let dependency_todo =
		support::dogfood_issue(config.service_id(), "issue-dependency", "PUB-942A", "Todo", &[]);
	let dependent_todo = support::with_blocker(
		support::dogfood_issue(config.service_id(), "issue-dependent", "PUB-942B", "Todo", &[]),
		"PUB-942A",
		"Todo",
	);
	let tracker =
		DogfoodTracker::default().with_issues([dependency_todo.clone(), dependent_todo.clone()]);
	let dry_run_report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("PUB-942B"), String::from("PUB-942A")],
		true,
		false,
	)
	.expect("issue-batch dry-run should classify controlled fixtures");

	assert!(dry_run_report.dry_run);
	assert!(!dry_run_report.persisted);
	assert_eq!(dry_run_report.counts.ready, 1);
	assert_eq!(dry_run_report.counts.blocked, 1);
	assert!(store.list_execution_programs(config.service_id()).expect("programs").is_empty());

	let apply_report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("PUB-942B"), String::from("PUB-942A")],
		false,
		true,
	)
	.expect("issue-batch apply should persist the internal program");

	assert!(apply_report.persisted);
	assert!(tracker.label_additions().is_empty());
	assert_eq!(
		store
			.list_program_issue_mappings(config.service_id(), &apply_report.program_id)
			.expect("program issue mappings should list")
			.len(),
		2
	);

	assert_initial_issue_batch_dispatch(&tracker, &config, &workflow, &store, &dependency_todo);

	let mut dependency_done =
		support::dogfood_issue(config.service_id(), "issue-dependency", "PUB-942A", "Done", &[]);

	dependency_done.id.clone_from(&dependency_todo.id);
	tracker.upsert_issue(dependency_done);

	let mut dependent_unblocked = support::with_blocker(
		support::dogfood_issue(config.service_id(), "issue-dependent", "PUB-942B", "Todo", &[]),
		"PUB-942A",
		"Done",
	);

	dependent_unblocked.id.clone_from(&dependent_todo.id);
	tracker.upsert_issue(dependent_unblocked);

	assert_unlocked_issue_batch_dispatch(&tracker, &config, &workflow, &store, &dependent_todo);
}

fn assert_initial_issue_batch_dispatch(
	tracker: &DogfoodTracker,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	store: &StateStore,
	dependency_todo: &TrackerIssue,
) {
	let first_selection = orchestrator::select_execution_program_run_candidate_with_summary(
		tracker,
		config,
		workflow,
		store,
		&[],
	)
	.expect("first program scheduler selection should choose only the ready node");
	let selected = first_selection
		.selected
		.expect("ready dependency node should be selected for direct dispatch");

	assert_eq!(first_selection.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, dependency_todo.id);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
	assert!(tracker.label_additions().is_empty());
	assert!(tracker.label_removals().is_empty());

	let first_snapshot =
		orchestrator::build_live_operator_status_snapshot(tracker, config, workflow, store, 10)
			.expect("first status snapshot should build");
	let first_program =
		first_snapshot.execution_programs.first().expect("program status should surface");

	assert_eq!(first_program.status, "blocked");
	assert_eq!(first_program.intake_kind.as_deref(), Some("issue_batch_intake"));
	assert_eq!(first_program.ready_count, 1);
	assert_eq!(first_program.queued_count, 0);
	assert_eq!(first_program.blocked_count, 1);
	assert!(
		first_program
			.node_readbacks
			.iter()
			.any(|node| node.reason_codes.contains(&String::from("dependency_not_terminal")))
	);
}

fn assert_unlocked_issue_batch_dispatch(
	tracker: &DogfoodTracker,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	store: &StateStore,
	dependent_todo: &TrackerIssue,
) {
	let second_selection = orchestrator::select_execution_program_run_candidate_with_summary(
		tracker,
		config,
		workflow,
		store,
		&[],
	)
	.expect("second program scheduler selection should unlock the downstream node");
	let selected =
		second_selection.selected.expect("dependent node should be selected for direct dispatch");

	assert_eq!(second_selection.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, dependent_todo.id);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
	assert!(tracker.label_additions().is_empty());
	assert!(tracker.label_removals().is_empty());

	let second_snapshot =
		orchestrator::build_live_operator_status_snapshot(tracker, config, workflow, store, 10)
			.expect("second status snapshot should build");
	let second_program =
		second_snapshot.execution_programs.first().expect("program status should remain visible");
	let rendered_status = orchestrator::render_operator_status(&second_snapshot);

	assert_eq!(second_program.status, "ready", "{rendered_status}");
	assert_eq!(second_program.completed_count, 1);
	assert_eq!(second_program.ready_count, 1);
	assert_eq!(second_program.queued_count, 0);
	assert_eq!(second_program.blocked_count, 0);
	assert!(rendered_status.contains("Execution Programs"));
	assert!(rendered_status.contains("mapped_issues=PUB-942A, PUB-942B"));
	assert!(!rendered_status.contains("private_evidence"));
}
