mod program_reconciler {
use std::collections::BTreeSet;

use crate::execution_program::{
	ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionLinearIssueMapping,
	ExecutionProgram, ExecutionProgramDependency, ExecutionProgramNode, ExecutionProgramNodeStage,
	ExecutionQueueIntent,
};
use crate::state::StateStore;
use crate::tracker::{self, TrackerIssue, TrackerLabel};
use crate::orchestrator;

use crate::orchestrator::tests::{
	FakeTracker, sample_issue_with_project_slug_and_sort_fields, temp_project_layout,
};

#[test]
fn selects_ready_node_for_direct_program_dispatch_without_queue_label_mutation() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = program_reconciler_issue("issue-ready", "PUB-201", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node(
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
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn unlocks_downstream_node_when_dependency_reaches_terminal_state() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let dependency_todo = program_reconciler_issue("issue-dependency", "PUB-202", "Todo", &[]);
	let dependency_done = program_reconciler_issue("issue-dependency", "PUB-202", "Done", &[]);
	let dependent = program_reconciler_issue("issue-dependent", "PUB-203", "Todo", &[]);
	let dependent_node = program_reconciler_node(
		"node-dependent",
		&dependent,
		ExecutionQueueIntent::ReadyToQueue,
	)
	.with_dependencies([ExecutionProgramDependency::new("node-dependency")
		.expect("dependency should build")])
	.expect("dependency should attach");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![
				program_reconciler_node(
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

#[test]
fn active_conflict_domain_holds_peer_node() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let active_issue =
		program_reconciler_issue("issue-active", "PUB-204", "In Progress", &[active_label.as_str()]);
	let peer_issue = program_reconciler_issue("issue-peer", "PUB-205", "Todo", &[]);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![
				program_reconciler_node(
					"node-active",
					&active_issue,
					ExecutionQueueIntent::Active,
				)
				.with_conflict_domains([conflict.clone()])
				.expect("active conflict should attach"),
				program_reconciler_node(
					"node-peer",
					&peer_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict])
				.expect("peer conflict should attach"),
			]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![active_issue, peer_issue]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("conflict program dispatch selection should succeed");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn needs_attention_node_is_not_selected_even_with_stale_queue_label() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = program_reconciler_queue_label();
	let issue = program_reconciler_issue(
		"issue-attention",
		"PUB-206",
		"Todo",
		&[queue_label.as_str(), "decodex:needs-attention"],
	);

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node_with_mapping(
				"node-attention",
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
	.expect("attention program dispatch selection should succeed");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn excluded_ready_issue_is_not_selected() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = program_reconciler_issue("issue-excluded", "PUB-207", "Todo", &[]);

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node_with_mapping(
				"node-excluded",
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
		&[issue.id.as_str()],
	)
	.expect("excluded program dispatch selection should succeed");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);
	assert!(tracker.label_removals.borrow().is_empty());
}

fn program_reconciler_program(nodes: Vec<ExecutionProgramNode>) -> ExecutionProgram {
	ExecutionProgram::from_issue_batch_intake(
		"program-reconciler",
		"pubfi",
		"program-reconciler-fingerprint",
		"Program reconciler test intake.",
		nodes,
	)
	.expect("program should build")
}

fn program_reconciler_node(
	node_id: &str,
	issue: &TrackerIssue,
	queue_intent: ExecutionQueueIntent,
) -> ExecutionProgramNode {
	program_reconciler_node_with_mapping(node_id, issue, queue_intent)
}

fn program_reconciler_node_with_mapping(
	node_id: &str,
	issue: &TrackerIssue,
	queue_intent: ExecutionQueueIntent,
) -> ExecutionProgramNode {
	let mapping = program_reconciler_mapping(issue);

	ExecutionProgramNode::new(
		node_id,
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve {}.", issue.identifier),
		queue_intent,
	)
	.expect("node should build")
	.with_acceptance_expectations([format!("{} is executable.", issue.identifier)])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run focused validation.")])
	.expect("validation should attach")
	.with_linear_issue(mapping)
	.expect("issue mapping should attach")
}

fn program_reconciler_mapping(issue: &TrackerIssue) -> ExecutionLinearIssueMapping {
	ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)
		.expect("mapping should build")
		.with_active_label(issue.has_label(&tracker::automation_active_label("pubfi")))
		.with_opt_out_label(issue.has_label("decodex:manual-only"))
		.with_needs_attention_label(issue.has_label("decodex:needs-attention"))
		.with_open_tracker_blockers(!issue.blockers.is_empty())
}

fn program_reconciler_issue(
	id: &str,
	identifier: &str,
	state_name: &str,
	labels: &[&str],
) -> TrackerIssue {
	let mut issue = sample_issue_with_project_slug_and_sort_fields(
		id,
		identifier,
		"pubfi",
		state_name,
		&[],
		Some(3),
		"2026-06-12T00:00:00.000Z",
	);

	issue.labels = labels
		.iter()
		.copied()
		.collect::<BTreeSet<_>>()
		.into_iter()
		.enumerate()
		.map(|(index, label)| TrackerLabel {
			id: format!("label-current-{index}"),
			name: label.to_owned(),
		})
		.collect();

	issue
}

fn program_reconciler_queue_label() -> String {
	tracker::automation_queue_label("pubfi")
}

}
