use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	tracker,
};

#[test]
fn machine_only_fenced_descriptions_fail_normal_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let cases = [
		(
			"single json fence",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\",\n  \"id\": \"ptr-1\"\n}\n```",
		),
		(
			"multiple json fences",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n```\n\n```json\n{\n  \"schema\": \"opaque-pointer/2\"\n}\n```",
		),
		("four backtick json fence", "````json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n````"),
		("tilde json fence", "~~~json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n~~~"),
	];

	for (case_name, description) in cases {
		let mut issue = tests::sample_issue("Todo", &[]);

		issue.description = description.to_owned();

		let tracker = FakeTracker::new(vec![issue.clone()]);

		assert!(
			!orchestrator::issue_passes_dispatch_policy(
				&tracker,
				&issue,
				&workflow,
				&tracker::automation_queue_label(TEST_SERVICE_ID),
				false,
			)
			.expect("dispatch policy should succeed"),
			"normal dispatch should reject {case_name} without a human briefing surface"
		);
	}
}
