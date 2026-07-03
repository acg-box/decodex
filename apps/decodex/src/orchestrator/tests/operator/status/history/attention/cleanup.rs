use crate::orchestrator::tests::operator::status::{self, FakeTracker, StateStore, orchestrator};

#[test]
fn live_status_terminal_cleanup_demotes_unleased_protocol_observed_current_lane() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-xy-952",
		"XY-952",
		"Done",
		&[],
		Some(3),
		"2026-06-16T08:50:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_path = config.worktree_root().join("XY-952");

	state_store
		.record_run_attempt("xy-952-attempt-2-1781598614", &issue.id, 2, "running")
		.expect("stale running attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"y/elf-xy-952",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_event(
			"xy-952-attempt-2-1781598614",
			1,
			"item/tool/call",
			"{\"tool\":\"issue_progress_checkpoint\"}",
		)
		.expect("protocol evidence should record");

	status::seed_local_linear_execution_events(
		&state_store,
		&status::successful_linear_execution_history_comments_with_cleanup(&issue),
	);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot.projects[0].current_lane_count, 0);
	assert_eq!(snapshot.projects[0].running_lane_count, 0);
	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].issue_identifier.as_deref(), Some("XY-952"));
	assert_eq!(snapshot.history_lanes[0].latest_run.run_id, "xy-952-attempt-2-1781598614");
	assert_eq!(snapshot.history_lanes[0].latest_run.status, "cleanup_complete");
	assert_eq!(snapshot.history_lanes[0].latest_run.phase, "completed");
	assert_eq!(snapshot.history_lanes[0].latest_run.current_operation, "ledger_outcome");
	assert_eq!(snapshot.history_lanes[0].ledger_outcome.final_outcome, "cleanup_complete");
	assert_eq!(snapshot_json["current_lanes"].as_array().map(Vec::len), Some(0));
	assert!(rendered.contains("Current lanes: 0"));
	assert!(rendered.contains("Running lanes: 0"));
	assert!(rendered.contains("\nCurrent Lanes\n- none\n"));
	assert!(rendered.contains("outcome: cleanup_complete"));
}
