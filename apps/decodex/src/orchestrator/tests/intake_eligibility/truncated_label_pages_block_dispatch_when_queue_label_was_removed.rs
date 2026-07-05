use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	tracker,
};

#[test]
fn truncated_label_pages_block_dispatch_when_queue_label_was_removed() {
	let (_, _, workflow) = tests::temp_project_layout();
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.labels_complete = false;

	issue.labels.retain(|label| label.name != queue_label.as_str());

	let tracker = FakeTracker::new(vec![]);

	assert!(
		!orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&queue_label,
			false,
		)
		.expect("dispatch policy should succeed"),
		"dispatch should re-check queue membership server-side when the local label page is truncated"
	);
}
