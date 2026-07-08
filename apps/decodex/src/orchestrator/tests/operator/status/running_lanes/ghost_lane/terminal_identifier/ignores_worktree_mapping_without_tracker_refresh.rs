use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, TERMINAL_GUARDED_RUN_STATUS, orchestrator,
};

#[test]
fn ignores_worktree_mapping_without_tracker_refresh() {
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
