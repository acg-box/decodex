use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	state::StateStore,
};

#[test]
fn eligibility_uses_state_label_blocker_and_lease_rules() {
	let (_, _, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let eligible_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![eligible_issue.clone()]);
	let opted_out_issue = tests::sample_issue("Todo", &["decodex:manual-only"]);
	let needs_attention_issue = tests::sample_issue("Todo", &["decodex:needs-attention"]);
	let mut blocked_issue = tests::sample_issue("Todo", &[]);

	blocked_issue.blockers = vec![tests::sample_blocker("issue-2", "PUB-102", "In Progress")];

	let mut unblocked_issue = tests::sample_issue("Todo", &[]);

	unblocked_issue.blockers = vec![tests::sample_blocker("issue-3", "PUB-103", "Done")];

	let wrong_state_issue = tests::sample_issue("In Progress", &[]);

	assert!(
		orchestrator::is_issue_eligible(
			&tracker,
			&eligible_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&opted_out_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&needs_attention_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&blocked_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		orchestrator::is_issue_eligible(
			&tracker,
			&unblocked_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&wrong_state_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);

	state_store
		.upsert_lease("pubfi", "issue-1", "run-1", "In Progress")
		.expect("lease should record");

	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&eligible_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
}
