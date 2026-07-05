use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator, tracker,
};

#[test]
fn live_status_does_not_count_done_history_attention_without_retained_ownership() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-pub-1549",
		"PUB-1549",
		"Done",
		&[],
		Some(3),
		"2026-06-12T01:56:00Z",
	);

	issue.labels.retain(|label| label.name != queue_label);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let comments = status::retained_partial_progress_linear_execution_history_comments(&issue);

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-mono-pub-1549",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should remember previous lane ownership");
	state_store
		.record_run_attempt("pub-1549-attempt-1-1781240781", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	state_store
		.clear_worktree(&issue.id)
		.expect("completed lane cleanup should clear local worktree");
	tracker.issue_comments.borrow_mut().insert(issue.id.clone(), comments);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let lane = snapshot.history_lanes.first().expect("history lane should exist");

	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert!(snapshot.queued_candidates.is_empty());
	assert_eq!(lane.issue_state.as_deref(), Some("Done"));
	assert_eq!(lane.active_label_present, Some(false));
	assert_eq!(lane.needs_attention_label_present, Some(false));
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(lane.latest_run.status, "needs_attention");
}
