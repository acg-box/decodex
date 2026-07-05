use crate::tracker::records::{
	self, LinearExecutionEventIdentity, LinearExecutionEventRecord,
	tests::{
		support,
		support::{AllowingClassifier, SuspiciousWordClassifier, UnavailableClassifier},
	},
};

#[test]
fn privacy_classifier_allows_public_projection_text() {
	let mut record = support::progress_record();

	record.phase = Some(String::from("implementing"));
	record.summary = Some(String::from("Execution phase: implementing."));

	let projection =
		records::linear_execution_event_public_projection("", &record, &AllowingClassifier);

	assert!(!projection.classifier_withheld_text);
	assert_eq!(projection.record.summary.as_deref(), Some("Execution phase: implementing."));

	records::validate_linear_execution_event_record(&projection.record)
		.expect("allowed projection should remain valid");
}

#[test]
fn privacy_classifier_replaces_suspicious_required_summary_and_skips_optional_text() {
	let mut record = support::progress_record();

	record.phase = Some(String::from("implementing"));
	record.summary = Some(String::from("Execution phase includes private family detail."));
	record.raw_error = Some(String::from("raw private family detail"));

	let projection = records::linear_execution_event_public_projection(
		"comment has private family detail",
		&record,
		&SuspiciousWordClassifier,
	);

	assert!(projection.classifier_withheld_text);
	assert_eq!(
		projection.record.summary.as_deref(),
		Some(crate::tracker::records::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY)
	);
	assert!(projection.record.raw_error.is_none());
	assert_eq!(
		projection.body,
		crate::tracker::records::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY
	);

	records::validate_linear_execution_event_record(&projection.record)
		.expect("classified projection should remain valid");
}

#[test]
fn unavailable_privacy_classifier_fails_closed_with_fixed_public_fields() {
	let mut record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "decodex",
			issue_id: "issue-id",
			issue_identifier: "XY-519",
			run_id: "xy-519-attempt-1",
			attempt_number: 1,
		},
		"terminal_failure",
		String::from("2026-05-25T00:00:00Z"),
		"anchor",
	);

	record.error_class = Some(String::from("repo_gate_failed"));
	record.next_action = Some(String::from("inspect the failed command"));
	record.blockers = Some(vec![String::from("repo gate failed")]);
	record.evidence = Some(vec![String::from("cargo make test failed")]);

	let projection =
		records::linear_execution_event_public_projection("", &record, &UnavailableClassifier);

	assert!(projection.classifier_withheld_text);
	assert_eq!(
		projection.record.next_action.as_deref(),
		Some(crate::tracker::records::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION)
	);
	assert_eq!(
		projection.record.blockers.as_deref(),
		Some(
			[String::from(crate::tracker::records::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL)]
				.as_slice()
		)
	);
	assert_eq!(
		projection.record.evidence.as_deref(),
		Some(
			[String::from(crate::tracker::records::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL)]
				.as_slice()
		)
	);

	records::validate_linear_execution_event_record(&projection.record)
		.expect("unavailable classifier fallback should remain valid");
}
