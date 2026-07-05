use crate::tracker::{
	privacy_classifier::PublicProjectionPrivacyClassifier,
	records::{PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY, public_projection::text},
};

pub(in crate::tracker::records::public_projection) fn classify_public_comment_body(
	body: &str,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> (String, bool) {
	if body.trim().is_empty() {
		return (String::new(), false);
	}
	if text::classifier_allows(privacy_classifier, "body", body) {
		return (body.to_owned(), false);
	}

	(String::from(PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY), true)
}
