use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		ChildRunRef, StateStore, TempDir, orchestrator, seed_runtime_failure_lane_claim,
	},
};

#[test]
fn exited_child_cleanup_keeps_other_run_lease_and_worktree_mapping() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let removed_worktree_path = temp_dir.path().join("removed-lane");

	seed_runtime_failure_lane_claim(&state_store, &issue.id, "other-run");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.record_run_attempt("other-run", &issue.id, 2, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&removed_worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
		false,
	)
	.expect("orphaned child cleanup should succeed");

	assert_eq!(
		state_store
			.claim_for_lane("pubfi", &issue.id)
			.expect("claim lookup")
			.expect("claim should remain attached to the other run")
			.run_id(),
		"other-run"
	);
	assert_eq!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.expect("worktree mapping should remain")
			.worktree_path(),
		removed_worktree_path.as_path()
	);
}
