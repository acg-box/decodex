use crate::tracker::privacy_classifier::{
	PublicProjectionPrivacyClassification, PublicProjectionPrivacyClassifier,
};

use super::{LinearExecutionEventPublicProjection, LinearExecutionEventRecord};

pub(crate) const PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY: &str =
	"Public summary withheld by local privacy classifier.";
pub(crate) const PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION: &str =
	"Public action withheld by local privacy classifier; review private Decodex evidence.";
pub(crate) const PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL: &str =
	"Public detail withheld by local privacy classifier.";
pub(crate) const PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY: &str =
	"Public comment details withheld by local privacy classifier.";

pub(crate) fn linear_execution_event_public_projection(
	body: &str,
	record: &LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> LinearExecutionEventPublicProjection {
	let (body, body_withheld) = classify_public_comment_body(body, privacy_classifier);
	let (record, record_withheld) =
		classify_linear_execution_event_record(record, privacy_classifier);

	LinearExecutionEventPublicProjection {
		body,
		record,
		classifier_withheld_text: body_withheld || record_withheld,
	}
}

fn classify_public_comment_body(
	body: &str,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> (String, bool) {
	if body.trim().is_empty() {
		return (String::new(), false);
	}
	if classifier_allows(privacy_classifier, "body", body) {
		return (body.to_owned(), false);
	}

	(String::from(PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY), true)
}

fn classify_linear_execution_event_record(
	record: &LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> (LinearExecutionEventRecord, bool) {
	let mut record = record.clone();
	let event_type = record.event_type.clone();
	let mut withheld = false;

	classify_optional_text_field(
		&mut record.summary,
		"summary",
		event_requires_summary(&event_type),
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY,
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_field(
		&mut record.focus,
		"focus",
		false,
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL,
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_field(
		&mut record.next_action,
		"next_action",
		event_requires_next_action(&event_type),
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION,
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_field(
		&mut record.failed_command,
		"failed_command",
		false,
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL,
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_field(
		&mut record.raw_error,
		"raw_error",
		false,
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL,
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_items(
		&mut record.blockers,
		"blockers",
		event_requires_items(&event_type, "blockers"),
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_items(
		&mut record.evidence,
		"evidence",
		event_requires_items(&event_type, "evidence"),
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_items(
		&mut record.verification,
		"verification",
		false,
		privacy_classifier,
		&mut withheld,
	);

	(record, withheld)
}

fn classify_optional_text_field(
	value: &mut Option<String>,
	field_name: &str,
	required: bool,
	fallback: &str,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
	withheld: &mut bool,
) {
	let Some(current) = value.as_deref() else {
		return;
	};

	if classifier_allows(privacy_classifier, field_name, current) {
		return;
	}

	*withheld = true;

	if required {
		*value = Some(fallback.to_owned());
	} else {
		*value = None;
	}
}

fn classify_optional_text_items(
	values: &mut Option<Vec<String>>,
	field_name: &str,
	required: bool,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
	withheld: &mut bool,
) {
	let Some(current_values) = values.take() else {
		return;
	};
	let mut retained = Vec::new();

	for value in current_values {
		if classifier_allows(privacy_classifier, field_name, &value) {
			retained.push(value);
		} else {
			*withheld = true;
		}
	}

	if retained.is_empty() {
		*values = required.then(|| vec![String::from(PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL)]);
	} else {
		*values = Some(retained);
	}
}

fn classifier_allows(
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
	field_name: &str,
	value: &str,
) -> bool {
	matches!(
		privacy_classifier.classify_public_projection_text(field_name, value),
		PublicProjectionPrivacyClassification::Allow
	)
}

fn event_requires_summary(event_type: &str) -> bool {
	matches!(
		event_type,
		"run_started"
			| "intake"
			| "progress_checkpoint"
			| "review_handoff"
			| "repair_handoff"
			| "review_handoff_rebind"
			| "review_handoff_adopt"
			| "landed"
			| "closeout"
			| "cleanup_complete"
	)
}

fn event_requires_next_action(event_type: &str) -> bool {
	matches!(event_type, "needs_attention" | "terminal_failure")
}

fn event_requires_items(event_type: &str, field_name: &str) -> bool {
	matches!(
		(event_type, field_name),
		(
			"needs_attention"
				| "terminal_failure"
				| "review_handoff_rebind"
				| "review_handoff_adopt",
			"evidence",
		) | ("needs_attention" | "terminal_failure", "blockers")
	)
}
