use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker,
};

#[test]
fn candidate_selection_does_not_requery_queue_label_for_truncated_candidates() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![issue.clone()]);
	let mut truncated_issue = issue.clone();

	truncated_issue.labels_complete = false;

	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![truncated_issue],
		&workflow,
		&state_store,
		TEST_SERVICE_ID,
	)
	.expect("candidate selection should succeed")
	.expect("queue candidate should remain selectable");

	assert_eq!(selected.identifier, issue.identifier);
	assert!(tracker.label_queries.borrow().is_empty());
}
