use crate::tracker::records::{self, LinearExecutionEventIdentity, tests::support};

#[test]
fn validates_public_progress_checkpoint_projection() {
	let record = records::render_progress_checkpoint_public_projection(
		LinearExecutionEventIdentity {
			service_id: "decodex",
			issue_id: "issue-id",
			issue_identifier: "XY-519",
			run_id: "xy-519-attempt-1",
			attempt_number: 1,
		},
		String::from("2026-05-25T00:00:00Z"),
		"implementing",
		Some("y/decodex-xy-519"),
		Some(".worktrees/XY-519"),
		Some("https://github.com/hack-ink/decodex/pull/42"),
	);

	records::validate_linear_execution_event_record(&record)
		.expect("public collaboration identifiers should validate");

	assert_eq!(record.summary.as_deref(), Some("Execution phase: implementing."));
	assert!(record.focus.is_none());
	assert!(record.next_action.is_none());
	assert!(record.evidence.is_none());
}

#[test]
fn rejects_private_progress_checkpoint_fields_in_linear_projection() {
	for field_name in ["focus", "next_action", "blockers", "evidence", "verification"] {
		let mut record = support::progress_record();

		record.phase = Some(String::from("implementing"));
		record.summary = Some(String::from("Execution phase: implementing."));

		match field_name {
			"focus" => record.focus = Some(String::from("private focus")),
			"next_action" => record.next_action = Some(String::from("private action")),
			"blockers" => record.blockers = Some(vec![String::from("private blocker")]),
			"evidence" => record.evidence = Some(vec![String::from("private evidence")]),
			"verification" => {
				record.verification = Some(vec![String::from("private verification")]);
			},
			_ => unreachable!("test field names are exhaustive"),
		}

		let error = records::validate_linear_execution_event_record(&record)
			.expect_err("private progress fields should not serialize");

		assert!(error.contains("belongs in private execution events"));
	}
}
