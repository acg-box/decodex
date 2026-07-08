use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionProgram,
		ExecutionProgramNode, ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	orchestrator::tests::operator::status::{
		self, FakeTracker, ReviewLifecycleHandoffFixture, StateStore, orchestrator,
		text::program_readback,
	},
};

#[test]
fn operator_status_snapshot_surfaces_program_intake_and_node_readbacks() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	program_readback::seed_program_readback_status(&state_store, &config);

	let snapshot =
		program_readback::build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let program_json = program_readback::program_readback_json(&snapshot);

	program_readback::assert_program_readback_summary(program);
	program_readback::assert_program_readback_json(&program_json);
	program_readback::assert_program_node_readbacks(program, &program_json);
}

#[test]
fn marks_retained_worktree_active_without_conflict_domain() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = "issue-retained-no-conflict";
	let node = program_readback::status_program_node(
		"node-retained-no-conflict",
		issue_id,
		"PUB-1598",
		"Todo",
		ExecutionQueueIntent::ReadyToQueue,
	);
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-retained-no-conflict",
		config.service_id(),
		"program-retained-no-conflict-fingerprint",
		"Read retained no-conflict Program node.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");
	state_store
		.upsert_worktree(
			config.service_id(),
			issue_id,
			"x/pubfi-pub-1598",
			&config.repo_root().display().to_string(),
		)
		.expect("retained worktree should persist");

	let snapshot =
		program_readback::build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let node = program.node_readbacks.first().expect("retained node should surface");

	assert_eq!(program.status, "active");
	assert_eq!(program.active_count, 1);
	assert_eq!(program.dispatchable_count, 0);
	assert_eq!(node.lifecycle_state, "active");
	assert_eq!(node.dispatch_action, None);
	assert!(node.reason_codes.contains(&String::from("current_lane_present")));
	assert!(!node.reason_codes.contains(&String::from("conflict_domain_occupied")));
}

#[test]
fn prefers_post_review_owner_over_stale_active_label() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = "issue-post-review";
	let issue_identifier = "PUB-946";
	let branch_name = "x/pubfi-pub-946";
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict domain should build");
	let node = program_readback::status_program_active_node(
		"node-post-review",
		issue_id,
		issue_identifier,
		"In Review",
	)
	.with_conflict_domains([conflict])
	.expect("conflict domain should attach");
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-post-review-owner",
		config.service_id(),
		"program-post-review-owner-fingerprint",
		"Track post-review owner readback.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");
	state_store
		.upsert_review_lifecycle_handoff_fixture(
			config.service_id(),
			issue_id,
			&ReviewLifecycleHandoffFixture::new(
				"pub-946-attempt-1",
				1,
				branch_name,
				"https://github.com/hack-ink/pubfi/pull/946",
				"main",
				branch_name,
				"1111111111111111111111111111111111111111",
			),
		)
		.expect("review lifecycle should persist");

	let snapshot =
		program_readback::build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let node = program.node_readbacks.first().expect("post-review node should surface");

	assert_eq!(program.status, "active");
	assert_eq!(program.active_count, 1);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(node.lifecycle_state, "post_review");
	assert_eq!(node.readiness_state, "blocked");
	assert!(node.reason_codes.contains(&String::from("mapped_issue_post_review_owner")));
	assert!(!node.reason_codes.contains(&String::from("mapped_issue_active_label_present")));
	assert!(!node.reason_codes.contains(&String::from("conflict_domain_occupied")));
	assert_eq!(
		node.reasons,
		vec![String::from(
			"Review & Landing owns this issue until post-review landing or closeout finishes",
		)]
	);
	assert_eq!(
		node.next_action,
		"Continue the retained post-review lifecycle before dispatching this program node."
	);
}

#[test]
fn operator_status_program_readback_refreshes_live_tracker_issue_mapping() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let stale_mapping =
		program_readback::status_program_issue_mapping("issue-live-refresh", "PUB-1597", "Todo")
			.with_needs_attention_label(true);
	let node = ExecutionProgramNode::new(
		"node-live-refresh",
		ExecutionProgramNodeStage::Runtime,
		"Close stale Program attention after the mapped issue is terminal.",
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_acceptance_expectations(["The mapped issue reflects live tracker state."])
	.expect("acceptance should attach")
	.with_validation_expectations(["Build operator status."])
	.expect("validation should attach")
	.with_linear_issue(stale_mapping)
	.expect("stale mapping should attach");
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-live-refresh",
		config.service_id(),
		"program-live-refresh-fingerprint",
		"Refresh live Program issue metadata.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let live_issue = status::sample_issue_with_sort_fields(
		"issue-live-refresh",
		"PUB-1597",
		"Done",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![live_issue]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.status, "completed");
	assert_eq!(program.completed_count, 1);
	assert_eq!(program.needs_attention_count, 0);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(program.dispatchable_count, 0);
	assert!(
		program.node_readbacks.is_empty(),
		"terminal refreshed Program nodes should not render stale attention readbacks"
	);
	assert_eq!(tracker.refresh_queries.borrow().len(), 1);
	assert_eq!(tracker.refresh_queries.borrow()[0], vec![String::from("issue-live-refresh")]);
}
