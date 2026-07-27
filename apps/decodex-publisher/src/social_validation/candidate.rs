//! social_candidate/v1 schema validation.

use crate::social_validation::{self, Map, SOCIAL_POST_MODES, SOCIAL_POST_PRIORITIES, Value};

pub(super) fn validate_social_candidate(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_candidate",
		&[
			"audience",
			"candidate_text",
			"caveats",
			"channel",
			"claims",
			"decision",
			"evidence_notes",
			"media_refs",
			"mode",
			"next_steps",
			"priority",
			"repo",
			"schema",
			"slug",
			"source_refs",
			"target_account",
		],
		errors,
	);

	for field in ["slug", "repo", "audience"] {
		if !social_validation::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if social_validation::string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
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
	if !social_validation::matches_one_of(entry.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors.push(format!(
			"priority must be one of {}",
			social_validation::choices(SOCIAL_POST_PRIORITIES)
		));
	}

	social_validation::validate_social_post_text(entry.get("candidate_text"), errors);

	validate_social_candidate_source_refs(entry.get("source_refs"), errors);

	social_validation::validate_non_empty_string_list(
		entry.get("evidence_notes"),
		"evidence_notes",
		errors,
	);
	social_validation::validate_social_post_claims(entry.get("claims"), errors);

	validate_social_candidate_decision(entry.get("decision"), errors);

	for field in ["caveats", "media_refs", "next_steps"] {
		social_validation::validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_candidate_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		refs,
		"source_refs",
		&["release_deltas", "signals", "upstream_impacts", "upstream_reviews", "urls"],
		errors,
	);
	let has_refs = ["upstream_reviews", "upstream_impacts", "signals", "release_deltas", "urls"]
		.iter()
		.any(|field| {
			refs.get(*field)
				.is_some_and(|value| !social_validation::is_empty_or_missing_array(Some(value)))
		});

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, signals, release_deltas, or urls"
				.into(),
		);
	}

	let uses_radar_inputs = ["upstream_reviews", "release_deltas"]
		.iter()
		.any(|field| social_validation::non_empty_array(refs.get(*field)).is_some());

	if uses_radar_inputs
		&& social_validation::non_empty_array(refs.get("upstream_impacts")).is_none()
	{
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff for Radar-derived social candidates"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !social_validation::is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "signals", "release_deltas"] {
		social_validation::validate_optional_string_list(
			refs.get(field),
			&format!("source_refs.{field}"),
			errors,
		);
	}
}

fn validate_social_candidate_decision(decision: Option<&Value>, errors: &mut Vec<String>) {
	let Some(decision) = decision.and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		decision,
		"decision",
		&["idempotency_key", "reason", "worthiness"],
		errors,
	);

	if !social_validation::matches_one_of(decision.get("worthiness"), &["publish", "skip"]) {
		errors.push("decision.worthiness must be one of ['publish', 'skip']".into());
	}

	for field in ["reason", "idempotency_key"] {
		if !social_validation::is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}
}
