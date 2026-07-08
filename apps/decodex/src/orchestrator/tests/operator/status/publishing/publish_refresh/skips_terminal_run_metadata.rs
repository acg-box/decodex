use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn skips_terminal_run_metadata() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-355",
		"Done",
		&[],
		Some(3),
		"2026-04-29T10:11:00Z",
	);
	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear connector is rate limited: Rate limit exceeded. Only 2500 requests are allowed per 1 hour.",
	);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"y/decodex-xy-355",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember project ownership");
	state_store
		.record_run_attempt("xy-355-attempt-1", &issue.id, 1, "succeeded")
		.expect("successful attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");

	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[],
		&[],
	)
	.expect("terminal-only publish should avoid Linear metadata refresh");

	assert_eq!(snapshot.history_lanes.len(), 1);
	assert!(
		!snapshot
			.warnings
			.iter()
			.any(|warning| warning == orchestrator::TRACKER_RATE_LIMIT_WARNING),
		"terminal-only publish should not enter backoff from run metadata"
	);
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"control-plane publish should not refresh terminal recent/history run metadata"
	);
}
