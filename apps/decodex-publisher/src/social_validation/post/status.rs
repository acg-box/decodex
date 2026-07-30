use crate::social_validation::{self, Map, SOCIAL_BLOCK_REASONS, Value};

pub(super) fn validate_social_post_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	let status = social_validation::string_field(entry, "status");

	match status {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") =>
			validate_reason_object(entry.get("failure"), "failure", &["reason", "details"], errors),
		Some("skipped") => validate_reason_object(entry.get("skip"), "skip", &["reason"], errors),
		_ => {},
	}
	validate_exclusive_status_payload(entry, status, errors);
}

fn validate_exclusive_status_payload(
	entry: &Map<String, Value>,
	status: Option<&str>,
	errors: &mut Vec<String>,
) {
	let expected = match status {
		Some("published") => "publication",
		Some("blocked") => "block",
		Some("failed") => "failure",
		Some("skipped") => "skip",
		_ => return,
	};

	for field in ["publication", "block", "failure", "skip"] {
		if field != expected && entry.get(field).is_some() {
			errors.push(format!(
				"{field} must be absent when status is {}",
				status.unwrap_or_default()
			));
		}
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication must be an object when status is published".into());

		return;
	};
	social_validation::validate_exact_keys(
		publication,
		"publication",
		&[
			"account_verified",
			"create_response_sha256",
			"identity_response_sha256",
			"made_with_ai",
			"post_id",
			"posted_at",
			"publication_lineage_sha256",
			"published_urls",
			"publisher",
			"read_response_sha256",
			"recorded_cost_ceiling_microusd",
			"verified_account",
			"verified_user_id",
			"xurl_app",
			"xurl_version",
		],
		errors,
	);

	if !social_validation::is_non_empty_string(publication.get("posted_at")) {
		errors.push("publication.posted_at must be a non-empty string".into());
	}
	if social_validation::string_field(publication, "publisher") != Some("xurl") {
		errors.push("publication.publisher must be xurl".into());
	}

	social_validation::validate_rfc3339_field(publication, "posted_at", errors);

	if publication.get("account_verified").and_then(Value::as_bool) != Some(true) {
		errors.push("publication.account_verified must be true".into());
	}
	if !publication.get("made_with_ai").is_some_and(Value::is_boolean) {
		errors.push("publication.made_with_ai must be a boolean".into());
	}
	if !publication
		.get("post_id")
		.and_then(Value::as_str)
		.is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
	{
		errors.push("publication.post_id must contain only digits".into());
	}
	if social_validation::string_field(publication, "xurl_app") != Some("default") {
		errors.push("publication.xurl_app must be default".into());
	}
	if social_validation::string_field(publication, "verified_account") != Some("decodexspace") {
		errors.push("publication.verified_account must be decodexspace".into());
	}
	if !publication
		.get("verified_user_id")
		.and_then(Value::as_str)
		.is_some_and(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
	{
		errors.push("publication.verified_user_id must contain only digits".into());
	}
	if social_validation::string_field(publication, "xurl_version") != Some("1.3.1") {
		errors.push("publication.xurl_version must be exactly 1.3.1".into());
	}
	if !publication
		.get("recorded_cost_ceiling_microusd")
		.and_then(Value::as_u64)
		.is_some_and(|value| matches!(value, 30_000 | 35_000 | 40_000))
	{
		errors.push(
			"publication.recorded_cost_ceiling_microusd must be 30000, 35000, or 40000".into(),
		);
	}
	for field in [
		"identity_response_sha256",
		"create_response_sha256",
		"publication_lineage_sha256",
		"read_response_sha256",
	] {
		if !publication.get(field).and_then(Value::as_str).is_some_and(valid_sha256) {
			errors.push(format!("publication.{field} must be a lowercase SHA-256 digest"));
		}
	}
	if !publication.get("published_urls").is_some_and(|urls| {
		social_validation::is_https_string_array(urls)
			&& !social_validation::is_empty_or_missing_array(Some(urls))
			&& urls.as_array().is_some_and(|urls| {
				urls.len() == 1
					&& urls.iter().all(|url| {
						url.as_str().is_some_and(|url| {
							url.starts_with("https://x.com/decodexspace/status/")
						})
					})
			})
	}) {
		errors.push(
			"publication.published_urls must contain exactly one decodexspace X status URL".into(),
		);
	}
}

fn valid_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(block) = entry.get("block").and_then(Value::as_object) else {
		errors.push("block must be an object when status is blocked".into());

		return;
	};
	social_validation::validate_exact_keys(block, "block", &["operator_notice", "reason"], errors);

	if !social_validation::matches_one_of(block.get("reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!(
			"block.reason must be one of {}",
			social_validation::choices(SOCIAL_BLOCK_REASONS)
		));
	}
	if !social_validation::is_non_empty_string(block.get("operator_notice")) {
		errors.push("block.operator_notice must be a non-empty string".into());
	}
}

fn validate_reason_object(
	value: Option<&Value>,
	label: &str,
	required_fields: &[&str],
	errors: &mut Vec<String>,
) {
	let Some(object) = value.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object when status is {label}"));

		return;
	};
	social_validation::validate_exact_keys(object, label, required_fields, errors);

	for field in required_fields {
		if !social_validation::is_non_empty_string(object.get(*field)) {
			errors.push(format!("{label}.{field} must be a non-empty string"));
		}
	}
}
