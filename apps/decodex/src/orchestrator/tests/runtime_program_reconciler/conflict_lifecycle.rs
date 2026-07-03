use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionQueueIntent,
	},
	orchestrator::{
		self,
		tests::{self, FakeTracker, runtime_program_reconciler::support},
	},
	state::{ReviewHandoffMarker, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn active_conflict_domain_holds_peer_node() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let active_issue = support::program_reconciler_issue(
		"issue-active",
		"PUB-204",
		"In Progress",
		&[active_label.as_str()],
	);
	let peer_issue = support::program_reconciler_issue("issue-peer", "PUB-205", "Todo", &[]);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node(
					"node-active",
					&active_issue,
					ExecutionQueueIntent::Active,
				)
				.with_conflict_domains([conflict.clone()])
				.expect("active conflict should attach"),
				support::program_reconciler_node(
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
fn post_review_lifecycle_holds_program_node_and_peer_conflict_domain() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let post_review_issue = support::program_reconciler_issue(
		"issue-post-review",
		"PUB-206",
		"In Review",
		&[active_label.as_str()],
	);
	let peer_issue = support::program_reconciler_issue("issue-peer", "PUB-207", "Todo", &[]);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node(
					"node-post-review",
					&post_review_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict.clone()])
				.expect("post-review conflict should attach"),
				support::program_reconciler_node(
					"node-peer",
					&peer_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict])
				.expect("peer conflict should attach"),
			]),
		)
		.expect("program should persist");
	store
		.upsert_review_handoff_marker(
			config.service_id(),
			&post_review_issue.id,
			&ReviewHandoffMarker::new(
				"pub-206-attempt-1",
				1,
				"x/pubfi-pub-206",
				"https://github.com/hack-ink/pubfi/pull/206",
				"main",
				"x/pubfi-pub-206",
				"1111111111111111111111111111111111111111",
			),
		)
		.expect("review lifecycle should persist");

	let tracker = FakeTracker::new(vec![post_review_issue, peer_issue]);
	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("post-review lifecycle should hold program dispatch");

	assert!(selection.selected.is_none());
	assert_eq!(selection.summary.dispatchable_nodes, 0);
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn live_reconciliation_clears_missing_orphaned_mapping_before_program_selection() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::program_reconciler_issue("issue-orphaned", "PUB-208", "Todo", &[]);
	let missing_worktree_path = config.worktree_root().join(&issue.identifier);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-208",
			&missing_worktree_path.display().to_string(),
		)
		.expect("orphaned mapping should persist");
	store
		.upsert_execution_program(
			config.service_id(),
			support::program_reconciler_program(vec![
				support::program_reconciler_node(
					"node-orphaned",
					&issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict])
				.expect("conflict should attach"),
			]),
		)
		.expect("program should persist");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let blocked = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("stale mapping should evaluate");

	assert!(blocked.selected.is_none());
	assert!(store.worktree_for_issue(&issue.id).expect("worktree lookup should succeed").is_some());

	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	orchestrator::reconcile_project_state(&tracker, &config, &workflow, &store, &worktree_manager)
		.expect("project reconciliation should succeed");

	assert!(store.worktree_for_issue(&issue.id).expect("worktree lookup should succeed").is_none());

	let ready = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("program dispatch selection should recover");
	let selected = ready.selected.expect("node should be selected");

	assert_eq!(ready.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, issue.id);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
}
