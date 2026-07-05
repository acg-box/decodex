use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	state::StateStore,
	tracker,
};

#[test]
fn claimed_issue_still_passes_post_claim_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.try_acquire_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease acquisition should succeed");

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&tracker::automation_queue_label(TEST_SERVICE_ID),
			false,
		)
		.expect("dispatch policy should succeed"),
		"post-claim policy should ignore the caller's own lease"
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("pre-claim eligibility should still reject leased issues")
	);
}
