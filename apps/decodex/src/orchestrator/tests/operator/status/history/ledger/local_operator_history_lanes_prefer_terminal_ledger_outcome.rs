use crate::orchestrator::tests::operator::status::{
	self, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn local_operator_history_lanes_prefer_terminal_ledger_outcome() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-799",
		"Done",
		&[],
		Some(3),
		"2026-06-08T04:12:00Z",
	);
	let local_comments = status::successful_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-799",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember project ownership");
	state_store
		.record_run_attempt("xy-799-attempt-1-1780888320", &issue.id, 1, "failed")
		.expect("stale failed attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");

	status::seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert!(!snapshot.recent_runs.is_empty());
	assert_eq!(lane.latest_run.status, "failed");
	assert_eq!(lane.latest_run.attempt_status, "failed");
	assert_eq!(
		lane.latest_run
			.loop_status
			.as_ref()
			.expect("terminal history should keep loop readback")
			.summary,
		"terminal lifecycle: failed"
	);
	assert_eq!(lane.ledger_outcome.ledger_status, "partial");
	assert_eq!(lane.ledger_outcome.final_outcome, "execution_log");
	assert_eq!(lane.ledger_outcome.final_event_type.as_deref(), Some("closeout"));
	assert_eq!(lane.attempts.len(), 1);
	assert_eq!(lane.attempts[0].status, "failed");
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "failed");
	assert_eq!(
		snapshot_json["history_lanes"][0]["latest_run"]["loop_status"]["summary"],
		"terminal lifecycle: failed"
	);
	assert_eq!(snapshot_json["history_lanes"][0]["attempts"][0]["status"], "failed");
	assert!(
		!snapshot_json["recent_runs"]
			.as_array()
			.expect("recent runs should be an array")
			.is_empty()
	);
}
