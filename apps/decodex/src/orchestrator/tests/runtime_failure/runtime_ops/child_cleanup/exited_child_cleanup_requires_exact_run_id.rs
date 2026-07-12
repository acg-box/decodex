use crate::orchestrator::tests::{
	self,
	runtime_failure::{ChildRunRef, StateStore, orchestrator, seed_runtime_failure_lane_claim},
};

#[test]
fn exited_child_cleanup_requires_exact_run_id() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	seed_runtime_failure_lane_claim(&state_store, &issue.id, "other-run");
	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");

	orchestrator::clear_orphaned_daemon_child_state(
		&state_store,
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		true,
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
			.run_attempt("other-run")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"running"
	);
}
