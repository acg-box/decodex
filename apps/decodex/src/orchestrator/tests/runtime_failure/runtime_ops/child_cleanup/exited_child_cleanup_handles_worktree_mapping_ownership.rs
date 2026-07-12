use crate::orchestrator::tests::{
	self,
	runtime_failure::{
		ChildRunRef, StateStore, TempDir, fs, orchestrator, seed_runtime_failure_lane_claim,
	},
};

#[test]
fn exited_child_cleanup_handles_worktree_mapping_ownership() {
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("Done", &[]);
		let removed_worktree_path = temp_dir.path().join("removed-lane");

		seed_runtime_failure_lane_claim(&state_store, &issue.id, "run-1");
		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store.update_run_status("run-1", "succeeded").expect("run status should update");
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
		.expect("removed worktree cleanup should succeed");

		assert!(
			state_store.claim_for_lane("pubfi", &issue.id).expect("claim lookup").is_none()
		);
		assert!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should exist")
				.status(),
			"succeeded"
		);
	}
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("In Review", &[]);
		let existing_worktree_path = temp_dir.path().join("retained-lane");

		fs::create_dir_all(&existing_worktree_path).expect("worktree path should exist");

		seed_runtime_failure_lane_claim(&state_store, &issue.id, "run-1");
		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&existing_worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
			false,
		)
		.expect("existing worktree cleanup should succeed");

		assert_eq!(
			state_store
				.worktree_for_issue(&issue.id)
				.expect("worktree lookup should succeed")
				.expect("worktree mapping should remain")
				.worktree_path(),
			existing_worktree_path.as_path()
		);
	}
}
