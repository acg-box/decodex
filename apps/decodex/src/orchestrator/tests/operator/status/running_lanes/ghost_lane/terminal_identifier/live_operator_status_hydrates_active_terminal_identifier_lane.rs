use crate::{
	orchestrator::tests::operator::status::{
		running_lanes,
		running_lanes::{FakeTracker, StateStore, TERMINAL_GUARDED_RUN_STATUS, orchestrator},
	},
	tracker,
};

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
