mod program_reconciler {
use color_eyre::Report;
use rusqlite::Connection;
use std::{collections::BTreeSet, path::Path};

use crate::agent::AppServerTransportFailure;
use crate::execution_program::{
	ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionLinearIssueMapping,
	ExecutionProgram, ExecutionProgramDependency, ExecutionProgramNode, ExecutionProgramNodeStage,
	ExecutionQueueIntent,
};
use crate::loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind};
use crate::state::{ReviewHandoffMarker, StateStore};
use crate::tracker::{self, TrackerIssue, TrackerLabel};
use crate::worktree::WorktreeManager;
use crate::orchestrator::{self, IssueDispatchMode, IssueRunPlan};

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
fn retryable_failed_start_cleanup_releases_program_node_for_retry() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let ready_issue = program_reconciler_issue("issue-retry", "PUB-211", "Todo", &[]);
	let active_issue = program_reconciler_issue(
		"issue-retry",
		"PUB-211",
		"In Progress",
		&[active_label.as_str()],
	);
	let cleaned_issue = program_reconciler_issue("issue-retry", "PUB-211", "Todo", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&ready_issue.identifier, false).expect("worktree should exist");
	let run_id = String::from("pub-211-attempt-1-123");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node(
				"node-retry",
				&ready_issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");
	store
		.upsert_worktree(
			config.service_id(),
			&ready_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.record_run_attempt(&run_id, &ready_issue.id, 1, "failed")
		.expect("run attempt should record");

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![active_issue.clone()],
		vec![vec![active_issue.clone()], vec![cleaned_issue.clone()]],
	);
	let issue_run = IssueRunPlan {
		issue: active_issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: active_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Program,
		attempt_number: 1,
		run_id: run_id.clone(),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected before thread start."),
		"thread/start",
		true,
	));

	orchestrator::handle_failure(&tracker, &config, &workflow, &store, &issue_run, &error)
		.expect("retryable failed-start cleanup should succeed");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(ready_issue.id.clone(), String::from("state-todo"))),
		"retryable failed-start cleanup should return the issue to the startable failure state"
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[(ready_issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(
		store.worktree_for_issue(&ready_issue.id).expect("worktree lookup should work").is_none(),
		"no-diff failed-start cleanup should clear the retained worktree mapping"
	);
	assert!(
		store.lease_for_issue(&ready_issue.id).expect("lease lookup should work").is_none(),
		"cleanup should not leave a live lease"
	);
	assert!(
		store
			.list_private_execution_events(config.service_id(), &ready_issue.id, &run_id, 1)
			.expect("private events should list")
			.iter()
			.any(|event| event.event_type() == "retryable_failed_start_cleanup"),
		"cleanup should leave private audit evidence after active ownership is removed"
	);

	let selection = orchestrator::select_execution_program_run_candidate_with_summary(
		&tracker,
		&config,
		&workflow,
		&store,
		&[],
	)
	.expect("next program pass should evaluate");
	let selected = selection.selected.expect("cleaned failed-start node should be selectable again");

	assert_eq!(selection.summary.dispatchable_nodes, 1);
	assert_eq!(selected.issue.id, ready_issue.id);
	assert_eq!(selected.dispatch_mode, IssueDispatchMode::Program);
}

#[test]
fn retryable_failed_start_cleanup_preserves_open_handoff_phase() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let ready_issue = program_reconciler_issue("issue-handoff", "PUB-212", "Todo", &[]);
	let active_issue = program_reconciler_issue(
		"issue-handoff",
		"PUB-212",
		"In Progress",
		&[active_label.as_str()],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&ready_issue.identifier, false).expect("worktree should exist");
	let current_run_id = String::from("pub-212-attempt-2-456");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node(
				"node-handoff",
				&ready_issue,
				ExecutionQueueIntent::ReadyToQueue,
			)]),
		)
		.expect("program should persist");
	store
		.upsert_worktree(
			config.service_id(),
			&ready_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_private_execution_event(
			config.service_id(),
			&ready_issue.id,
			"pub-212-attempt-1-123",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("open handoff phase should record");
	store
		.record_run_attempt(&current_run_id, &ready_issue.id, 2, "failed")
		.expect("current run attempt should record");

	let tracker = FakeTracker::new(vec![active_issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: active_issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: active_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Program,
		attempt_number: 2,
		run_id: current_run_id.clone(),
		retry_budget_base: 0,
	};
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected before thread start."),
		"thread/start",
		true,
	));

	orchestrator::handle_failure(&tracker, &config, &workflow, &store, &issue_run, &error)
		.expect("retryable failed-start writeback should preserve open handoff ownership");

	assert!(
		tracker.state_updates.borrow().is_empty(),
		"open handoff phases must keep the issue in active ownership"
	);
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"open handoff phases must not clear the active label"
	);
	assert!(
		store
			.worktree_for_issue(&ready_issue.id)
			.expect("worktree lookup should work")
			.is_some(),
		"open handoff phases must retain the worktree mapping"
	);
	assert!(
		store
			.list_private_execution_events(config.service_id(), &ready_issue.id, &current_run_id, 2)
			.expect("private events should list")
			.iter()
			.all(|event| event.event_type() != "retryable_failed_start_cleanup"),
		"open handoff phases must not be audited as cleaned failed-start ownership"
	);
}

#[test]
fn removed_flat_goal_contract_fields_migrate_before_direct_program_selection() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_field_issue =
		program_reconciler_issue("issue-removed-flat-contract", "PUB-209", "Todo", &[]);
	let ready_issue = program_reconciler_issue("issue-ready", "PUB-210", "Todo", &[]);
	let contract = program_reconciler_accepted_contract();
	let removed_field_program = ExecutionProgram::from_accepted_contract(
		"program-removed-flat-contract",
		config.service_id(),
		&contract,
		vec![program_reconciler_node(
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
		vec![program_reconciler_node(
			"node-ready",
			&ready_issue,
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("current program should build");

	store
		.upsert_execution_program(config.service_id(), removed_field_program)
		.expect("removed-field program should persist");

	insert_removed_flat_decision_contract(
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
fn post_review_lifecycle_holds_program_node_and_peer_conflict_domain() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(config.service_id());
	let post_review_issue = program_reconciler_issue(
		"issue-post-review",
		"PUB-206",
		"In Review",
		&[active_label.as_str()],
	);
	let peer_issue = program_reconciler_issue("issue-peer", "PUB-207", "Todo", &[]);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![
				program_reconciler_node(
					"node-post-review",
					&post_review_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict.clone()])
				.expect("post-review conflict should attach"),
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = program_reconciler_issue("issue-orphaned", "PUB-208", "Todo", &[]);
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
			program_reconciler_program(vec![
				program_reconciler_node(
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
	assert!(
		store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some()
	);

	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	orchestrator::reconcile_project_state(&tracker, &config, &workflow, &store, &worktree_manager)
		.expect("project reconciliation should succeed");

	assert!(
		store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none()
	);

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

#[test]
fn active_shared_lease_marks_program_node_active_without_self_conflict() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let issue = program_reconciler_issue("issue-active-claim", "PUB-1094", "Todo", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![program_reconciler_node(
				"node-active-claim",
				&issue,
				ExecutionQueueIntent::ReadyToQueue,
			)
			.with_conflict_domains([conflict])
			.expect("conflict should attach")]),
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

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&store,
		10,
	)
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let active_issue = program_reconciler_issue("issue-active-peer", "PUB-1094", "Todo", &[]);
	let peer_issue = program_reconciler_issue("issue-ready-peer", "PUB-1095", "Todo", &[]);
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict should build");

	store
		.upsert_execution_program(
			config.service_id(),
			program_reconciler_program(vec![
				program_reconciler_node(
					"node-active-peer",
					&active_issue,
					ExecutionQueueIntent::ReadyToQueue,
				)
				.with_conflict_domains([conflict.clone()])
				.expect("active conflict should attach"),
				program_reconciler_node(
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
		.upsert_lease(
			config.service_id(),
			&active_issue.id,
			"pub-1094-attempt-1",
			"In Progress",
		)
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

fn program_reconciler_accepted_contract() -> DecisionContract {
	let mut contract: DecisionContract = serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("decision contract fixture should deserialize");

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-17T00:00:00Z",
				"program-reconciler-test",
				Some(String::from("Accepted for program reconciler regression coverage.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

fn insert_removed_flat_decision_contract(
	state_path: &Path,
	service_id: &str,
	source_issue_id: Option<&str>,
	contract: &DecisionContract,
) {
	let mut removed_field_payload =
		serde_json::to_value(contract).expect("contract should encode as JSON");
	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated before dispatch."]),
	);

	let connection = Connection::open(state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
				project_id, contract_id, source_issue_id, status, payload_json, created_at,
				created_at_unix, updated_at, updated_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				service_id,
				contract.contract_id(),
				source_issue_id,
				contract.status().as_str(),
				serde_json::to_string(&removed_field_payload)
					.expect("removed-field payload should serialize"),
				"2026-06-17T00:00:00Z",
				1_i64,
				"2026-06-17T00:00:00Z",
				1_i64,
			],
		)
		.expect("removed-field decision contract row should insert");
}
}
