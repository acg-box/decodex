use crate::tracker::records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord};

#[test]
fn renders_attempt_number_as_run_sequence_in_comment_body() {
	let record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "decodex",
			issue_id: "issue-id",
			issue_identifier: "XY-519",
			run_id: "xy-519-attempt-4",
			attempt_number: 4,
		},
		"closeout",
		String::from("2026-05-25T00:00:00Z"),
		"anchor",
	);
	let body = records::render_linear_execution_event_comment_body(&record, Some(1));

	assert!(body.contains("Decodex execution event: closeout"));
	assert!(body.contains("- run_sequence_attempt: `4` (not retry-budget count)"));
	assert!(body.contains("- retry_budget_attempts_consumed: `1`"));
	assert!(!body.contains("- attempt:"));
}
