use crate::social_validation::{self, Map, SOCIAL_BLOCK_REASONS, Value};

pub(super) fn validate_social_post_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	match social_validation::string_field(entry, "status") {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") if !social_validation::is_non_empty_string(entry.get("failure_reason")) =>
			errors.push("failure_reason is required when status is failed".into()),
		Some("skipped") if !social_validation::is_non_empty_string(entry.get("skip_reason")) =>
			errors.push("skip_reason is required when status is skipped".into()),
		_ => {},
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication must be an object when status is published".into());

		return;
	};

	for field in ["posted_at", "publisher"] {
		if !social_validation::is_non_empty_string(publication.get(field)) {
			errors.push(format!("publication.{field} must be a non-empty string"));
		}
	}

	social_validation::validate_rfc3339_field(publication, "posted_at", errors);

	if !publication.get("account_verified").is_some_and(Value::is_boolean) {
		errors.push("publication.account_verified must be a boolean".into());
	}
	if !publication.get("made_with_ai").is_some_and(Value::is_boolean) {
		errors.push("publication.made_with_ai must be a boolean".into());
	}
	if publication.get("published_urls").is_some_and(|urls| {
		!social_validation::is_https_string_array(urls)
			|| social_validation::is_empty_or_missing_array(Some(urls))
	}) {
		errors.push("publication.published_urls must be a non-empty list of https URLs".into());
	}
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if !social_validation::matches_one_of(entry.get("block_reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!(
			"block_reason must be one of {}",
			social_validation::choices(SOCIAL_BLOCK_REASONS)
		));
	}
}
