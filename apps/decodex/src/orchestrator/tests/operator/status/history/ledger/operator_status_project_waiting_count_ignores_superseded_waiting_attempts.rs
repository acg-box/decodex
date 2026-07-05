use crate::orchestrator::tests::operator::status::{self, StateStore, orchestrator};

#[test]
fn operator_status_project_waiting_count_ignores_superseded_waiting_attempts() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-451",
		"Done",
		&[],
		Some(3),
		"2026-05-03T11:48:16Z",
	);

	state_store
		.record_run_attempt("xy-451-attempt-1-1777791228", &issue.id, 1, "stalled")
		.expect("stalled attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/decodex-xy-451",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_attempt("xy-451-attempt-4-1777808209", &issue.id, 4, "succeeded")
		.expect("successful attempt should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let grouped_lane = snapshot.history_lanes.first().expect("history lane should exist");

	assert_eq!(grouped_lane.attempt_count, 2);
	assert_eq!(grouped_lane.latest_run.run_id, "xy-451-attempt-4-1777808209");
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
}
