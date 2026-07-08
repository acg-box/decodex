use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn refreshes_current_lane_metadata() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"In Progress",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.record_run_attempt("xy-355-attempt-1", &issue.id, 1, "running")
		.expect("running attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "xy-355-attempt-1", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-355",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should remember project ownership");

	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[],
		&[],
	)
	.expect("current-lane publish should build");
	let refresh_queries = tracker.refresh_queries.borrow();

	assert!(
		refresh_queries.iter().any(|query| query.len() == 1 && query.first() == Some(&issue.id)),
		"current-lane publish should still refresh the current lane issue metadata"
	);
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].issue_identifier.as_deref(), Some("XY-355"));
	assert_eq!(snapshot.current_lanes[0].title.as_deref(), Some("Implement orchestration"));
	assert_eq!(snapshot.current_lanes[0].author.as_deref(), Some("Yvette"));

	let snapshot_json =
		orchestrator::operator_snapshot_json_value(&snapshot).expect("snapshot should project");

	assert_eq!(snapshot_json["presentation"]["schema"], "decodex.operator.presentation/1");
	assert_eq!(
		snapshot_json["presentation"]["current_lane_cards"][0]["run_id"],
		"xy-355-attempt-1"
	);
	assert_eq!(snapshot_json["presentation"]["current_lane_cards"][0]["title"], "XY-355");
	assert_eq!(
		snapshot_json["presentation"]["current_lane_cards"][0]["run"]["title"],
		"Implement orchestration"
	);
}
