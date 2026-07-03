use crate::{
	orchestrator::tests::operator::status::{
		running_lanes,
		running_lanes::{
			FakeTracker, ReviewPolicyCheckpointInput, StateStore, TERMINAL_GUARDED_RUN_STATUS,
			TEST_SERVICE_ID, orchestrator,
		},
	},
	tracker,
};

#[test]
fn live_operator_status_classifies_missing_issue_ghost_lane_for_runtime_recovery() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(run.run_id, "run-12");
	assert_eq!(run.issue_id, "PUB-012");
	assert_eq!(run.issue_identifier.as_deref(), Some("PUB-012"));
	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert!(run.lane_control_conditions.contains(&String::from("tracker_issue_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("worktree_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("private_evidence_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("review_lineage_missing")));
	assert_eq!(project.attention_count, 1);
	assert!(!rendered.contains("Record the independent Decodex Review checkpoint"));
	assert!(!rendered.contains("review-handoff"));
}

#[test]
fn live_operator_status_classifies_invalid_local_issue_id_as_ghost_lane() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");

	assert_eq!(run.issue_id, "PUB-012");
	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert!(run.lane_control_conditions.contains(&String::from("tracker_issue_missing")));
	assert!(
		!snapshot.warnings.iter().any(|warning| warning.contains("runtime_recovery_unavailable"))
	);
}

#[test]
fn live_operator_status_ignores_terminal_identifier_worktree_mapping_without_tracker_refresh() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let stale_issue_id = "PUB-001";
	let missing_worktree_path = config.worktree_root().join(stale_issue_id);

	state_store
		.record_run_attempt("run-01", stale_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			stale_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert!(snapshot.worktrees.is_empty());
	assert!(
		snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored"))
	);
	assert_eq!(history_lane.issue_id, stale_issue_id);
	assert_eq!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert_eq!(history_lane.ledger_outcome.final_outcome, "local_terminal_residue");
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.all(|issue_id| issue_id != stale_issue_id),
		"terminal local identifier id must not be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().all(|issue_id| issue_id != stale_issue_id),
		"terminal local identifier id must not be used for Linear ledger lookup"
	);
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("stale_terminal_local_worktree_mapping_ignored"));
	assert!(!rendered.contains("execution_ledger_status_unavailable"));
}

#[test]
fn live_operator_status_hydrates_terminal_identifier_history_with_review_checkpoint() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let protected_issue_id = "PUB-001";
	let issue = running_lanes::sample_issue_with_sort_fields(
		protected_issue_id,
		protected_issue_id,
		"In Review",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![issue]);
	let missing_worktree_path = config.worktree_root().join(protected_issue_id);

	state_store
		.record_run_attempt("run-01", protected_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			protected_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: protected_issue_id,
			run_id: "run-01",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert!(
		!snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored")),
		"review-authority mappings must not be classified as local residue"
	);
	assert!(
		snapshot.worktrees.iter().any(|worktree| worktree.issue_id == protected_issue_id),
		"review-authority worktree mapping must remain visible"
	);
	assert_ne!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.any(|issue_id| issue_id == protected_issue_id),
		"review-authority terminal identifier id must still be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().any(|issue_id| issue_id == protected_issue_id),
		"review-authority terminal identifier id must still be used for Linear ledger lookup"
	);
}

#[test]
fn live_operator_status_hydrates_active_terminal_identifier_lane() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_issue_id = "PUB-001";
	let issue = running_lanes::sample_issue_with_sort_fields(
		active_issue_id,
		active_issue_id,
		"In Progress",
		&[tracker::automation_active_label(config.service_id()).as_str()],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![issue]);
	let missing_worktree_path = config.worktree_root().join(active_issue_id);

	state_store
		.record_run_attempt("run-01", active_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_lease("pubfi", active_issue_id, "run-01", "In Progress")
		.expect("active lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			active_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("active worktree mapping should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert_eq!(history_lane.issue_id, active_issue_id);
	assert_ne!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert!(
		!snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored")),
		"active lanes must not be classified as local residue"
	);
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.any(|issue_id| issue_id == active_issue_id),
		"active terminal identifier id must still be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().any(|issue_id| issue_id == active_issue_id),
		"active terminal identifier id must still be used for Linear ledger lookup"
	);
}
