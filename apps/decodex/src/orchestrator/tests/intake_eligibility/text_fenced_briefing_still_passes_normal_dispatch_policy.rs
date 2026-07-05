use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	tracker,
};

#[test]
fn text_fenced_briefing_still_passes_normal_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.description =
		String::from("```text\nImplement the retained lane repair and keep scope tight.\n```");

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
		"human-readable fenced text should still count as a generic briefing surface"
	);
}
