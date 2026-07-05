#[cfg(test)]
use crate::tracker::{
	privacy_classifier::{
		PublicProjectionPrivacyClassification, PublicProjectionPrivacyClassifier,
	},
	records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

struct AllowingClassifier;
impl PublicProjectionPrivacyClassifier for AllowingClassifier {
	fn classify_public_projection_text(
		&self,
		_field_name: &str,
		_text: &str,
	) -> PublicProjectionPrivacyClassification {
		PublicProjectionPrivacyClassification::Allow
	}
}

struct SuspiciousWordClassifier;
impl PublicProjectionPrivacyClassifier for SuspiciousWordClassifier {
	fn classify_public_projection_text(
		&self,
		_field_name: &str,
		text: &str,
	) -> PublicProjectionPrivacyClassification {
		if text.contains("private family detail") {
			return PublicProjectionPrivacyClassification::Suspicious {
				reason: String::from("matched fake private phrase"),
			};
		}

		PublicProjectionPrivacyClassification::Allow
	}
}

struct UnavailableClassifier;
impl PublicProjectionPrivacyClassifier for UnavailableClassifier {
	fn classify_public_projection_text(
		&self,
		_field_name: &str,
		_text: &str,
	) -> PublicProjectionPrivacyClassification {
		PublicProjectionPrivacyClassification::Unavailable {
			reason: String::from("fake classifier unavailable"),
		}
	}
}

fn progress_record() -> LinearExecutionEventRecord {
	LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "decodex",
			issue_id: "issue-id",
			issue_identifier: "XY-519",
			run_id: "xy-519-attempt-1",
			attempt_number: 1,
		},
		"progress_checkpoint",
		String::from("2026-05-25T00:00:00Z"),
		"anchor",
	)
}

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
	let body = super::render_linear_execution_event_comment_body(&record, Some(1));

	assert!(body.contains("Decodex execution event: closeout"));
	assert!(body.contains("- run_sequence_attempt: `4` (not retry-budget count)"));
	assert!(body.contains("- retry_budget_attempts_consumed: `1`"));
	assert!(!body.contains("- attempt:"));
}

#[test]
fn validates_public_progress_checkpoint_projection() {
	let record = super::render_progress_checkpoint_public_projection(
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

	super::validate_linear_execution_event_record(&record)
		.expect("public collaboration identifiers should validate");

	assert_eq!(record.summary.as_deref(), Some("Execution phase: implementing."));
	assert!(record.focus.is_none());
	assert!(record.next_action.is_none());
	assert!(record.evidence.is_none());
}

#[test]
fn rejects_private_progress_checkpoint_fields_in_linear_projection() {
	for field_name in ["focus", "next_action", "blockers", "evidence", "verification"] {
		let mut record = progress_record();

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

		let error = super::validate_linear_execution_event_record(&record)
			.expect_err("private progress fields should not serialize");

		assert!(error.contains("belongs in private execution events"));
	}
}

#[test]
fn privacy_classifier_allows_public_projection_text() {
	let mut record = progress_record();

	record.phase = Some(String::from("implementing"));
	record.summary = Some(String::from("Execution phase: implementing."));

	let projection =
		super::linear_execution_event_public_projection("", &record, &AllowingClassifier);

	assert!(!projection.classifier_withheld_text);
	assert_eq!(projection.record.summary.as_deref(), Some("Execution phase: implementing."));

	super::validate_linear_execution_event_record(&projection.record)
		.expect("allowed projection should remain valid");
}

#[test]
fn privacy_classifier_replaces_suspicious_required_summary_and_skips_optional_text() {
	let mut record = progress_record();

	record.phase = Some(String::from("implementing"));
	record.summary = Some(String::from("Execution phase includes private family detail."));
	record.raw_error = Some(String::from("raw private family detail"));

	let projection = super::linear_execution_event_public_projection(
		"comment has private family detail",
		&record,
		&SuspiciousWordClassifier,
	);

	assert!(projection.classifier_withheld_text);
	assert_eq!(
		projection.record.summary.as_deref(),
		Some(super::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY)
	);
	assert!(projection.record.raw_error.is_none());
	assert_eq!(projection.body, super::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY);

	super::validate_linear_execution_event_record(&projection.record)
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
		super::linear_execution_event_public_projection("", &record, &UnavailableClassifier);

	assert!(projection.classifier_withheld_text);
	assert_eq!(
		projection.record.next_action.as_deref(),
		Some(super::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION)
	);
	assert_eq!(
		projection.record.blockers.as_deref(),
		Some([String::from(super::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL)].as_slice())
	);
	assert_eq!(
		projection.record.evidence.as_deref(),
		Some([String::from(super::PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL)].as_slice())
	);

	super::validate_linear_execution_event_record(&projection.record)
		.expect("unavailable classifier fallback should remain valid");
}
