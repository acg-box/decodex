use crate::tracker::{
	privacy_classifier::{
		PublicProjectionPrivacyClassification, PublicProjectionPrivacyClassifier,
	},
	records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

pub(in crate::tracker::records::tests) struct AllowingClassifier;
impl PublicProjectionPrivacyClassifier for AllowingClassifier {
	fn classify_public_projection_text(
		&self,
		_field_name: &str,
		_text: &str,
	) -> PublicProjectionPrivacyClassification {
		PublicProjectionPrivacyClassification::Allow
	}
}

pub(in crate::tracker::records::tests) struct SuspiciousWordClassifier;
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

pub(in crate::tracker::records::tests) struct UnavailableClassifier;
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

pub(in crate::tracker::records::tests) fn progress_record() -> LinearExecutionEventRecord {
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
