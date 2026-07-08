use crate::orchestrator::tests::operator::status::{
	self, OffsetDateTime, StateStore, orchestrator, state,
};

#[test]
fn ignores_superseded_retry_backoff() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-452",
		"Done",
		&[],
		Some(3),
		"2026-05-03T11:49:16Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.record_run_attempt("xy-452-attempt-1-1777791228", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/decodex-xy-452",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_schedule(
		&worktree_path,
		"xy-452-attempt-1-1777791228",
		1,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("retry schedule marker should write");

	state_store
		.record_run_attempt("xy-452-attempt-2-1777808209", &issue.id, 2, "succeeded")
		.expect("successful attempt should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let grouped_lane = snapshot.history_lanes.first().expect("history lane should exist");

	assert_eq!(grouped_lane.latest_run.run_id, "xy-452-attempt-2-1777808209");
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
	assert_eq!(snapshot.projects[0].connector_state, "ok");
}
