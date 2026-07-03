use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionQueueIntent,
	},
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn active_shared_lease_marks_program_node_active_without_self_conflict() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue("issue-active-claim", "PUB-1094", "Todo", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node(
					"node-active-claim",
					&issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict])
				.expect("conflict should attach"),
			]),
		)
		.expect("program should persist");
	store
		.record_run_attempt("pub-1094-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_lease(config.service_id(), &issue.id, "pub-1094-attempt-1", "In Progress")
		.expect("lease should record");
	store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("active claim should evaluate");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);

	let snapshot =
		orchestrator::build_live_operator_status_snapshot(&tracker, &config, &workflow, &store, 10)
			.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should render");
	let node = program.node_readbacks.first().expect("active node should render");

	assert_eq!(program.active_count, 1);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(program.dispatchable_count, 0);
	assert_eq!(node.lifecycle_state, "active");
	assert_eq!(node.readiness_state, "active");
	assert!(node.reason_codes.contains(&String::from("current_lane_present")));
	assert!(!node.reason_codes.contains(&String::from("conflict_domain_occupied")));
}

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
