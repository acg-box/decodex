use crate::tracker::{
	privacy_classifier::PublicProjectionPrivacyClassifier,
	records::{
		LinearExecutionEventRecord, PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION,
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL, PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY,
		public_projection::{requirements, text},
	},
};

pub(in crate::tracker::records::public_projection) fn classify_linear_execution_event_record(
	record: &LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> (LinearExecutionEventRecord, bool) {
	let mut record = record.clone();
	let event_type = record.event_type.clone();
	let mut withheld = false;

	classify_optional_text_field(
		&mut record.summary,
		"summary",
		requirements::event_requires_summary(&event_type),
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
		requirements::event_requires_next_action(&event_type),
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
		requirements::event_requires_items(&event_type, "blockers"),
		privacy_classifier,
		&mut withheld,
	);
	classify_optional_text_items(
		&mut record.evidence,
		"evidence",
		requirements::event_requires_items(&event_type, "evidence"),
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

	if text::classifier_allows(privacy_classifier, field_name, current) {
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
		if text::classifier_allows(privacy_classifier, field_name, &value) {
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
