//! social_post/v1 schema validation.

mod claims;
mod decision;
mod lifecycle;
mod source_refs;
mod status;
mod text;

use crate::social_validation::{self, Map, SOCIAL_POST_MODES, SOCIAL_POST_STATUSES, Value};
pub(in crate::social_validation) use status::{
	BrowserSessionRequirement, validate_browser_session,
};

pub(super) fn validate_social_post(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_post",
		&[
			"audience",
			"block",
			"browser_session",
			"browser_touched",
			"caveats",
			"channel",
			"claims",
			"controller_account",
			"decision",
			"evidence_notes",
			"failure",
			"media_refs",
			"mode",
			"post_lifecycle",
			"publication",
			"schema",
			"skip",
			"slug",
			"source_refs",
			"status",
			"target_account",
			"text",
		],
		errors,
	);

	for field in ["slug", "audience"] {
		if !social_validation::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_social_post_constants(entry, errors);
	validate_social_post_text(entry.get("text"), errors);
	validate_social_post_source_refs(entry.get("source_refs"), errors);

	social_validation::validate_non_empty_string_list(
		entry.get("evidence_notes"),
		"evidence_notes",
		errors,
	);
	validate_social_post_claims(entry.get("claims"), errors);
	validate_social_post_decision(entry, errors);
	validate_social_post_status_payload(entry, errors);
	validate_social_post_browser_evidence(entry, errors);
	validate_social_post_lifecycle(entry, errors);

	for field in ["caveats", "media_refs"] {
		social_validation::validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_post_browser_evidence(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let browser_touched = entry.get("browser_touched").and_then(Value::as_bool);

	if browser_touched.is_none() {
		errors.push("browser_touched must be a boolean".into());

		return;
	}
	if social_validation::string_field(entry, "status") == Some("published")
		&& browser_touched != Some(true)
	{
		errors.push("browser_touched must be true when status is published".into());
	}

	match browser_touched {
		Some(true) => status::validate_browser_session(
			entry.get("browser_session"),
			if social_validation::string_field(entry, "status") == Some("published") {
				BrowserSessionRequirement::Complete
			} else {
				BrowserSessionRequirement::Terminal
			},
			errors,
		),
		Some(false) if entry.get("browser_session").is_some() =>
			errors.push("browser_session must be absent when browser_touched is false".into()),
		_ => {},
	}
}

pub(super) fn validate_social_post_text(text: Option<&Value>, errors: &mut Vec<String>) {
	text::validate_social_post_text(text, errors);
}

pub(super) fn validate_social_post_claims(claims: Option<&Value>, errors: &mut Vec<String>) {
	claims::validate_social_post_claims(claims, errors);
}

fn validate_social_post_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	source_refs::validate_social_post_source_refs(refs, errors);
}

fn validate_social_post_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	decision::validate_social_post_decision(entry, errors);
}

fn validate_social_post_status_payload(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	status::validate_social_post_status_payload(entry, errors);
}

fn validate_social_post_lifecycle(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	lifecycle::validate_social_post_lifecycle(entry, errors);
}

fn validate_social_post_constants(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if social_validation::string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if social_validation::string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if social_validation::string_field(entry, "controller_account") != Some("hackink") {
		errors.push("controller_account must be hackink".into());
	}
	if !social_validation::matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors
			.push(format!("mode must be one of {}", social_validation::choices(SOCIAL_POST_MODES)));
	}
	if !social_validation::matches_one_of(entry.get("status"), SOCIAL_POST_STATUSES) {
		errors.push(format!(
			"status must be one of {}",
			social_validation::choices(SOCIAL_POST_STATUSES)
		));
	}
}
