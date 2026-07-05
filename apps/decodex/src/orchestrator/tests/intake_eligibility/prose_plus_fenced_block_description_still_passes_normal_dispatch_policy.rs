use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	tracker,
};

#[test]
fn prose_plus_fenced_block_description_still_passes_normal_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.description = String::from(
		"Implement the retained lane repair.\n\n```json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n```",
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&tracker::automation_queue_label(TEST_SERVICE_ID),
			false,
		)
		.expect("dispatch policy should succeed"),
		"dispatch should remain allowed when a generic briefing exists outside the fenced block"
	);
}
