use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	tracker,
};

#[test]
fn truncated_label_pages_do_not_block_queue_label_dispatch() {
	let (_, _, workflow) = tests::temp_project_layout();
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.labels_complete = false;

	issue.labels.retain(|label| label.name != queue_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![issue.clone()]);

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&queue_label,
			false,
		)
		.expect("dispatch policy should succeed"),
		"server-filtered queue membership should remain authoritative when the local label page is truncated"
	);
}
