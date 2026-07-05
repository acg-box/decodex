use crate::tracker::privacy_classifier::{
	PublicProjectionPrivacyClassification, PublicProjectionPrivacyClassifier,
};

pub(in crate::tracker::records::public_projection) fn classifier_allows(
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
	field_name: &str,
	value: &str,
) -> bool {
	matches!(
		privacy_classifier.classify_public_projection_text(field_name, value),
		PublicProjectionPrivacyClassification::Allow
	)
}
