use crate::orchestrator::tests::{
	self,
	runtime_failure::{ChildRunRef, StateStore, TempDir, fs, orchestrator},
};

#[test]
fn exited_child_cleanup_updates_status_and_retry_budget_by_interrupt_flag() {
	for (case_name, mark_interrupted, expected_status, expected_retry_budget) in
		[("clean exit", false, "running", 0), ("interrupted exit", true, "interrupted", 1)]
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("In Progress", &[]);

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");

		orchestrator::clear_orphaned_daemon_child_state(
			&state_store,
			ChildRunRef { issue_id: &issue.id, run_id: "run-1", attempt_number: 1 },
			mark_interrupted,
		)
		.expect(case_name);

		assert!(
			state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none()
		);
		assert_eq!(
			state_store
				.run_attempt("run-1")
				.expect("run attempt lookup should succeed")
				.expect("run attempt should exist")
				.status(),
			expected_status,
			"{case_name}"
		);
		assert_eq!(
			state_store
				.retry_budget_attempt_count(&issue.id)
				.expect("retry budget count should succeed"),
			expected_retry_budget,
			"{case_name}"
		);
	}
}

#[test]
fn exited_child_cleanup_handles_worktree_mapping_ownership() {
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("Done", &[]);
		let removed_worktree_path = temp_dir.path().join("removed-lane");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store.update_run_status("run-1", "succeeded").expect("run status should update");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");
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
			state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none()
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

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
			.expect("lease should record");
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

#[test]
fn exited_child_cleanup_requires_exact_run_id() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "other-run", "In Progress")
		.expect("lease should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		true,
	)
	.expect("orphaned child cleanup should succeed");

	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should remain attached to the other run")
			.run_id(),
		"other-run"
	);
	assert_eq!(
		state_store
			.run_attempt("other-run")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"running"
	);
}

#[test]
fn exited_child_cleanup_keeps_other_run_lease_and_worktree_mapping() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let removed_worktree_path = temp_dir.path().join("removed-lane");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.record_run_attempt("other-run", &issue.id, 2, "running")
		.expect("other run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "other-run", "In Progress")
		.expect("lease should record");
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
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should remain attached to the other run")
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
