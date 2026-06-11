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
fn applies_ready_queue_label_and_repeated_reconcile_is_idempotent() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = program_reconciler_issue("issue-ready", "PUB-201", "Todo", &[]);
	let queued_issue = program_reconciler_issue(
		"issue-ready",
		"PUB-201",
		"Todo",
		&[program_reconciler_queue_label().as_str()],
	);

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
	let summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("ready reconcile should succeed");

	assert_eq!(summary.labels_applied, 1);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-queued")])]
	);

	assert_program_owned_queue_label(&store, config.service_id(), &issue, true);

	let tracker = FakeTracker::new(vec![queued_issue.clone()]);
	let summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("idempotent reconcile should succeed");

	assert_eq!(summary.labels_applied, 0);
	assert_eq!(summary.labels_removed, 0);
	assert_eq!(summary.labels_retained, 1);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());

	assert_program_owned_queue_label(&store, config.service_id(), &queued_issue, true);
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
	let blocked_summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("blocked reconcile should succeed");

	assert_eq!(blocked_summary.labels_applied, 0);
	assert!(tracker.label_additions.borrow().is_empty());

	let tracker = FakeTracker::new(vec![dependency_done, dependent.clone()]);
	let ready_summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("unlock reconcile should succeed");

	assert_eq!(ready_summary.labels_applied, 1);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		&[(dependent.id.clone(), vec![String::from("label-queued")])]
	);
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
	let summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("conflict reconcile should succeed");

	assert_eq!(summary.labels_applied, 0);
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn needs_attention_removes_only_program_owned_queue_label() {
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
				true,
			)]),
		)
		.expect("program should persist");

	assert_program_owned_queue_label(&store, config.service_id(), &issue, true);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("attention reconcile should succeed");

	assert_eq!(summary.labels_removed, 1);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-queued")])]
	);

	assert_program_owned_queue_label(&store, config.service_id(), &issue, false);
}

#[test]
fn human_owned_queue_label_is_not_removed_when_node_blocks() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = program_reconciler_queue_label();
	let issue =
		program_reconciler_issue("issue-human", "PUB-207", "In Progress", &[queue_label.as_str()]);

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node_with_mapping(
				"node-human",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
				false,
			)]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let summary =
		orchestrator::reconcile_execution_program_queue_labels(&tracker, &config, &workflow, &store)
			.expect("human-owned reconcile should succeed");

	assert_eq!(summary.labels_removed, 0);
	assert!(tracker.label_removals.borrow().is_empty());

	assert_program_owned_queue_label(&store, config.service_id(), &issue, false);
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
	program_reconciler_node_with_mapping(node_id, issue, queue_intent, false)
}

fn program_reconciler_node_with_mapping(
	node_id: &str,
	issue: &TrackerIssue,
	queue_intent: ExecutionQueueIntent,
	program_owned_queue_label: bool,
) -> ExecutionProgramNode {
	let mapping = program_reconciler_mapping(issue, program_owned_queue_label);

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

fn program_reconciler_mapping(
	issue: &TrackerIssue,
	program_owned_queue_label: bool,
) -> ExecutionLinearIssueMapping {
	let mut mapping =
		ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)
			.expect("mapping should build");

	mapping = if program_owned_queue_label {
		mapping.with_program_owned_queue_label(true)
	} else {
		mapping.with_queue_label(issue.has_label(&program_reconciler_queue_label()))
	};

	mapping
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

fn assert_program_owned_queue_label(
	store: &StateStore,
	service_id: &str,
	issue: &TrackerIssue,
	expected_present: bool,
) {
	let records = store
		.program_queue_label_ownership_for_issue(
			service_id,
			&issue.id,
			&program_reconciler_queue_label(),
		)
		.expect("ownership records should load");

	assert_eq!(
		!records.is_empty(),
		expected_present,
		"program-owned queue-label evidence mismatch for {}",
		issue.identifier
	);
}
}
