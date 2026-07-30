//! social_post/v1 schema validation.

mod claims;
mod decision;
mod lifecycle;
mod source_refs;
mod status;
mod text;

pub(crate) use text::contains_link_like_text;

use crate::social_validation::{self, Map, SOCIAL_POST_MODES, SOCIAL_POST_STATUSES, Value};

pub(super) fn validate_social_post(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_post",
		&[
			"audience",
			"block",
			"caveats",
			"channel",
			"claims",
			"decision",
			"evidence_digests",
			"evidence_notes",
			"failure",
			"mode",
			"owner",
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
	validate_social_post_owner(entry.get("owner"), errors);
	validate_social_post_text(entry.get("text"), errors);
	validate_social_post_source_refs(entry.get("source_refs"), errors);
	if social_validation::string_field(entry, "status") == Some("published") {
		if entry.get("text").and_then(Value::as_array).map(Vec::len) != Some(1) {
			errors.push("published text must contain exactly one item".into());
		}
		let refs = entry.get("source_refs").and_then(Value::as_object);
		for field in ["reservations", "social_candidates"] {
			if refs.and_then(|refs| refs.get(field)).and_then(Value::as_array).map(Vec::len)
				!= Some(1)
			{
				errors.push(format!("published source_refs.{field} must contain exactly one item"));
			}
		}
		if entry
			.get("text")
			.and_then(Value::as_array)
			.and_then(|items| items.first())
			.and_then(Value::as_str)
			.is_none_or(|text| text.chars().count() < 80)
		{
			errors.push("published text item must contain at least 80 Unicode characters".into());
		}
	}

	social_validation::validate_non_empty_string_list(
		entry.get("evidence_notes"),
		"evidence_notes",
		errors,
	);
	validate_social_post_claims(
		entry.get("claims"),
		entry.get("source_refs"),
		entry.get("evidence_digests"),
		true,
		errors,
	);
	validate_social_post_decision(entry, errors);
	validate_social_post_status_payload(entry, errors);
	validate_social_post_lifecycle(entry, errors);

	social_validation::validate_optional_string_list(entry.get("caveats"), "caveats", errors);
}

fn validate_social_post_owner(owner: Option<&Value>, errors: &mut Vec<String>) {
	let Some(owner) = owner.and_then(Value::as_object) else {
		errors.push("owner must be an object".into());
		return;
	};
	social_validation::validate_exact_keys(owner, "owner", &["automation_id", "run_id"], errors);
	if social_validation::string_field(owner, "automation_id") != Some("decodex-xurl-publisher") {
		errors.push("owner.automation_id must be decodex-xurl-publisher".into());
	}
	if social_validation::string_field(owner, "run_id")
		.is_none_or(|value| !crate::social_publish::valid_run_id(value))
	{
		errors.push("owner.run_id must be a lowercase UUID".into());
	}
}

pub(super) fn validate_social_post_text(text: Option<&Value>, errors: &mut Vec<String>) {
	text::validate_social_post_text(text, errors);
}

pub(super) fn validate_social_post_claims(
	claims: Option<&Value>,
	source_refs: Option<&Value>,
	evidence_digests: Option<&Value>,
	allow_candidate_lineage: bool,
	errors: &mut Vec<String>,
) {
	claims::validate_social_post_claims(
		claims,
		source_refs,
		evidence_digests,
		allow_candidate_lineage,
		errors,
	);
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
