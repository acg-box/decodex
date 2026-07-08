use crate::orchestrator::tests::operator::status::{
	self, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn ignores_history_only_attention_without_current_owner() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-pub-1549",
		"PUB-1549",
		"Todo",
		&[],
		Some(3),
		"2026-06-12T01:56:00Z",
	);
	let local_comments =
		status::retained_partial_progress_linear_execution_history_comments(&issue);

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
		.expect("historical lane should not have current retained ownership");

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
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.current_lanes.len(), 0);
	assert_eq!(snapshot.queued_candidates.len(), 0);
	assert_eq!(snapshot.post_review_lanes.len(), 0);
	assert_eq!(snapshot.worktrees.len(), 0);
	assert_eq!(snapshot.projects[0].current_lane_count, 0);
	assert_eq!(snapshot.projects[0].running_lane_count, 0);
	assert_eq!(snapshot.projects[0].queued_candidate_count, 0);
	assert_eq!(snapshot.projects[0].post_review_lane_count, 0);
	assert_eq!(snapshot.projects[0].attention_count, 0);
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert_eq!(snapshot.projects[0].cleanup_blocked_count, 0);
	assert_eq!(snapshot.projects[0].cleanup_pending_count, 0);
	assert_eq!(lane.ledger_outcome.ledger_status, "present");
	assert_eq!(lane.ledger_outcome.final_outcome, "needs_attention");
	assert_eq!(lane.latest_run.status, "needs_attention");
	assert_eq!(lane.latest_run.phase, "needs_attention");
	assert!(!lane.latest_run.run_lease);
	assert_eq!(snapshot_json["projects"][0]["attention_count"], 0);
	assert_eq!(snapshot_json["worktrees"].as_array().map(Vec::len), Some(0));
	assert_eq!(snapshot_json["history_lanes"][0]["latest_run"]["status"], "needs_attention");
	assert!(rendered.contains("Current attention: 0"));
	assert!(rendered.contains("History-only terminal attention: 1"));
	assert!(rendered.contains(
		"Current attention action: none; terminal attention rows below are Run Ledger history only."
	));
	assert!(rendered.contains("outcome: needs_attention"));
}
