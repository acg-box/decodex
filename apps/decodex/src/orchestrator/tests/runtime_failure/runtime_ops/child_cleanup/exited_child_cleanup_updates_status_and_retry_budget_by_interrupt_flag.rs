use crate::orchestrator::tests::{
	self,
	runtime_failure::{ChildRunRef, StateStore, orchestrator},
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
