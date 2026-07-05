mod body;
mod constants;
mod record;
mod requirements;
mod text;

pub(crate) use self::constants::{
	PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION, PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY,
	PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL, PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY,
};

use crate::tracker::{
	privacy_classifier::PublicProjectionPrivacyClassifier,
	records::{LinearExecutionEventPublicProjection, LinearExecutionEventRecord},
};

pub(crate) fn linear_execution_event_public_projection(
	body: &str,
	record: &LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> LinearExecutionEventPublicProjection {
	let (body, body_withheld) = self::body::classify_public_comment_body(body, privacy_classifier);
	let (record, record_withheld) =
		self::record::classify_linear_execution_event_record(record, privacy_classifier);

	LinearExecutionEventPublicProjection {
		body,
		record,
		classifier_withheld_text: body_withheld || record_withheld,
	}
}
