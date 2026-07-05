use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionQueueIntent,
	},
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
};

#[test]
fn active_shared_lease_occupies_peer_conflict_domain() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_issue =
		support::program_reconciler_issue("issue-active-peer", "PUB-1094", "Todo", &[]);
	let peer_issue = support::program_reconciler_issue("issue-ready-peer", "PUB-1095", "Todo", &[]);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node(
					"node-active-peer",
					&active_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict.clone()])
				.expect("active conflict should attach"),
				support::program_reconciler_node(
					"node-ready-peer",
					&peer_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict])
				.expect("peer conflict should attach"),
			]),
		)
		.expect("program should persist");
	store
		.record_run_attempt("pub-1094-attempt-1", &active_issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_lease(config.service_id(), &active_issue.id, "pub-1094-attempt-1", "In Progress")
		.expect("lease should record");

	let tracker = FakeTracker::new(vec![active_issue.clone(), peer_issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("active claim should occupy peer conflict");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);

	let snapshot =
		orchestrator::build_live_operator_status_snapshot(&tracker, &config, &workflow, &store, 10)
			.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should render");
	let active_node = program
		.node_readbacks
		.iter()
		.find(|node| node.issue_identifier.as_deref() == Some(active_issue.identifier.as_str()))
		.expect("active node should render");
	let peer_node = program
		.node_readbacks
		.iter()
		.find(|node| node.issue_identifier.as_deref() == Some(peer_issue.identifier.as_str()))
		.expect("peer node should render");

	assert_eq!(program.active_count, 1);
	assert_eq!(program.blocked_count, 1);
	assert_eq!(program.dispatchable_count, 0);
	assert_eq!(active_node.lifecycle_state, "active");
	assert_eq!(active_node.readiness_state, "active");
	assert!(active_node.reason_codes.contains(&String::from("current_lane_present")));
	assert_eq!(peer_node.lifecycle_state, "blocked");
	assert_eq!(peer_node.readiness_state, "blocked");
	assert!(peer_node.reason_codes.contains(&String::from("conflict_domain_occupied")));
}
