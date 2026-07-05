use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, ReviewPolicyCheckpointInput, StateStore, TERMINAL_GUARDED_RUN_STATUS,
	TEST_SERVICE_ID, orchestrator,
};

#[test]
fn live_operator_status_hydrates_terminal_identifier_history_with_review_checkpoint() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let protected_issue_id = "PUB-001";
	let issue = running_lanes::sample_issue_with_sort_fields(
		protected_issue_id,
		protected_issue_id,
		"In Review",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![issue]);
	let missing_worktree_path = config.worktree_root().join(protected_issue_id);

	state_store
		.record_run_attempt("run-01", protected_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			protected_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: protected_issue_id,
			run_id: "run-01",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert!(
		!snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored")),
		"review-authority mappings must not be classified as local residue"
	);
	assert!(
		snapshot.worktrees.iter().any(|worktree| worktree.issue_id == protected_issue_id),
		"review-authority worktree mapping must remain visible"
	);
	assert_ne!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.any(|issue_id| issue_id == protected_issue_id),
		"review-authority terminal identifier id must still be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().any(|issue_id| issue_id == protected_issue_id),
		"review-authority terminal identifier id must still be used for Linear ledger lookup"
	);
}
