use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn reads_local_completed_ledger() {
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
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let local_comments = status::successful_linear_execution_history_comments_with_cleanup(&issue);

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

	status::seed_local_linear_execution_events(&state_store, &local_comments);

	let snapshot = orchestrator::build_operator_state_snapshot_for_publish(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
		&[],
		&[],
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(lane.ledger_outcome.ledger_status, "partial");
	assert_eq!(lane.ledger_outcome.final_outcome, "execution_log");
	assert_eq!(lane.ledger_outcome.final_event_type.as_deref(), Some("cleanup_complete"));
	assert_eq!(
		lane.ledger_outcome.pr_url.as_deref(),
		Some("https://github.com/hack-ink/decodex/pull/355")
	);
	assert_eq!(
		lane.ledger_outcome.commit_sha.as_deref(),
		Some("2222222222222222222222222222222222222222")
	);
	assert_eq!(lane.ledger_outcome.closeout_status, None);
	assert_eq!(lane.ledger_outcome.lifecycle_elapsed_seconds, Some(660));
	assert_eq!(lane.ledger_outcome.record_count, 6);
	assert_eq!(snapshot_json["history_lanes"][0]["ledger_outcome"]["ledger_status"], "partial");
	assert_eq!(
		snapshot_json["history_lanes"][0]["ledger_outcome"]["pr_url"],
		"https://github.com/hack-ink/decodex/pull/355"
	);
	assert!(
		tracker.comment_queries.borrow().is_empty(),
		"control-plane publish should use local execution events instead of replaying Linear comments"
	);
}
