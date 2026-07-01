//! social_candidate/v1 schema validation.

#[allow(clippy::wildcard_imports)] use super::*;

pub(super) fn validate_social_candidate(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "audience"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors.push(format!("priority must be one of {}", choices(SOCIAL_POST_PRIORITIES)));
	}

	validate_social_post_text(entry.get("candidate_text"), errors);
	validate_social_candidate_source_refs(entry.get("source_refs"), errors);
	validate_non_empty_string_list(entry.get("evidence_notes"), "evidence_notes", errors);
	validate_social_post_claims(entry.get("claims"), errors);
	validate_social_candidate_decision(entry.get("decision"), errors);

	for field in ["caveats", "media_refs", "next_steps"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_candidate_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = ["upstream_reviews", "upstream_impacts", "signals", "release_deltas", "urls"]
		.iter()
		.any(|field| refs.get(*field).is_some_and(|value| !is_empty_or_missing_array(Some(value))));

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, signals, release_deltas, or urls"
				.into(),
		);
	}

	let uses_radar_inputs = ["upstream_reviews", "release_deltas"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if uses_radar_inputs && non_empty_array(refs.get("upstream_impacts")).is_none() {
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff for Radar-derived social candidates"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "signals", "release_deltas"] {
		validate_optional_string_list(refs.get(field), &format!("source_refs.{field}"), errors);
	}
}

fn validate_social_candidate_decision(decision: Option<&Value>, errors: &mut Vec<String>) {
	let Some(decision) = decision.and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};

	if !matches_one_of(decision.get("worthiness"), &["defer", "publish", "skip"]) {
		errors.push("decision.worthiness must be one of ['defer', 'publish', 'skip']".into());
	}

	for field in ["reason", "idempotency_key"] {
		if !is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}
}
