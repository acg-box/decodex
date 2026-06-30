//! Social candidate, reservation, and post validation.

use serde_json::{Map, Value};

use super::{
	SIGNAL_CONFIDENCE,
	constants::{
		SOCIAL_BLOCK_REASONS, SOCIAL_POST_LIFECYCLE_STATES, SOCIAL_POST_MODES,
		SOCIAL_POST_PRIORITIES, SOCIAL_POST_STATUSES, SOCIAL_POST_WORTHINESS,
		SOCIAL_PUBLISH_RESERVATION_STATUSES,
	},
	support::{
		choices, is_empty_or_missing_array, is_https_string, is_https_string_array,
		is_non_empty_string, matches_one_of, non_empty_array, string_field,
		validate_non_empty_string_list, validate_optional_string_list, validate_rfc3339_field,
	},
};

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

pub(super) fn validate_social_candidate_source_refs(
	refs: Option<&Value>,
	errors: &mut Vec<String>,
) {
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

pub(super) fn validate_social_candidate_decision(
	decision: Option<&Value>,
	errors: &mut Vec<String>,
) {
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

pub(super) fn validate_social_publish_reservation(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "idempotency_key", "reserved_at", "expires_at", "day", "timezone"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_social_publish_reservation_constants(entry, errors);
	validate_social_publish_reservation_refs(entry.get("candidate_refs"), errors);
	validate_non_empty_string_list(entry.get("duplicate_keys"), "duplicate_keys", errors);
	validate_optional_string_list(entry.get("evidence_notes"), "evidence_notes", errors);
	validate_social_publish_reservation_owner(entry.get("owner"), errors);
	validate_rfc3339_field(entry, "reserved_at", errors);
	validate_rfc3339_field(entry, "expires_at", errors);
	validate_social_publish_reservation_status_payload(entry, errors);
}

pub(super) fn validate_social_publish_reservation_constants(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if string_field(entry, "controller_account") != Some("hackink") {
		errors.push("controller_account must be hackink".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("status"), SOCIAL_PUBLISH_RESERVATION_STATUSES) {
		errors.push(format!(
			"status must be one of {}",
			choices(SOCIAL_PUBLISH_RESERVATION_STATUSES)
		));
	}
}

pub(super) fn validate_social_publish_reservation_refs(
	refs: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("candidate_refs must be an object".into());

		return;
	};
	let has_refs = ["social_candidates", "urls"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push("candidate_refs must include social_candidates or urls".into());
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("candidate_refs.urls must be a list of https URLs".into());
	}

	validate_optional_string_list(
		refs.get("social_candidates"),
		"candidate_refs.social_candidates",
		errors,
	);
}

pub(super) fn validate_social_publish_reservation_owner(
	owner: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(owner) = owner else {
		return;
	};
	let Some(owner) = owner.as_object() else {
		errors.push("owner must be an object when present".into());

		return;
	};

	for field in ["automation_id", "branch", "pr_url", "run_id"] {
		if owner.get(field).is_some_and(|value| !is_non_empty_string(Some(value))) {
			errors.push(format!("owner.{field} must be non-empty when present"));
		}
	}

	if owner.get("pr_url").is_some_and(|value| !is_https_string(Some(value))) {
		errors.push("owner.pr_url must be an https URL when present".into());
	}
}

pub(super) fn validate_social_publish_reservation_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	match string_field(entry, "status") {
		Some("consumed") if !is_non_empty_string(entry.get("consumed_by_social_post")) =>
			errors.push("consumed_by_social_post is required when status is consumed".into()),
		Some("canceled" | "expired") if !is_non_empty_string(entry.get("release_reason")) =>
			errors.push("release_reason is required when status is canceled or expired".into()),
		_ => {},
	}
}

pub(super) fn validate_social_post(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "audience"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_social_post_constants(entry, errors);
	validate_social_post_text(entry.get("text"), errors);
	validate_social_post_source_refs(entry.get("source_refs"), errors);

	for field in ["evidence_notes", "claims"] {
		if non_empty_array(entry.get(field)).is_none() {
			errors.push(format!("{field} must be a non-empty list"));
		}
	}

	validate_social_post_claims(entry.get("claims"), errors);
	validate_social_post_decision(entry, errors);
	validate_social_post_status_payload(entry, errors);
	validate_social_post_lifecycle(entry, errors);

	for field in ["caveats", "media_refs"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

pub(super) fn validate_social_post_constants(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if string_field(entry, "controller_account") != Some("hackink") {
		errors.push("controller_account must be hackink".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("status"), SOCIAL_POST_STATUSES) {
		errors.push(format!("status must be one of {}", choices(SOCIAL_POST_STATUSES)));
	}
}

pub(super) fn validate_social_post_text(text: Option<&Value>, errors: &mut Vec<String>) {
	let Some(items) = non_empty_array(text) else {
		errors.push("text must be a non-empty list of X-sized strings".into());

		return;
	};

	for (index, item) in items.iter().enumerate() {
		let Some(text) = item.as_str() else {
			errors.push(format!("text[{index}] must be a string"));

			continue;
		};

		validate_social_post_text_item(text, index, errors);
	}
}

pub(super) fn validate_social_post_text_item(text: &str, index: usize, errors: &mut Vec<String>) {
	if text.is_empty() || text.len() > 280 {
		errors.push(format!("text[{index}] must be a non-empty X-sized string"));
	}
	if text.contains("Automated by @hackink") {
		errors.push(format!("text[{index}] must not include automation attribution"));
	}
	if text.len() > 260 && !text.contains("https://") {
		errors.push(format!(
			"text[{index}] longer than 260 characters must include an unavoidable direct source URL"
		));
	}

	let normalized = text.trim().to_ascii_lowercase();

	if normalized == "watching this"
		|| normalized.starts_with("watching this.")
		|| normalized.starts_with("tracking this.")
		|| normalized.contains("new release available")
	{
		errors.push(format!(
			"text[{index}] must name a concrete source-backed release, PR, protocol surface, workflow impact, or operator action"
		));
	}
}

pub(super) fn validate_social_post_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = [
		"reservations",
		"signals",
		"social_candidates",
		"upstream_impacts",
		"upstream_reviews",
		"urls",
	]
	.iter()
	.any(|field| non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push(
			"source_refs must include reservations, signals, social_candidates, upstream_impacts, upstream_reviews, or urls"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in
		["reservations", "signals", "social_candidates", "upstream_impacts", "upstream_reviews"]
	{
		validate_optional_string_list(refs.get(field), &format!("source_refs.{field}"), errors);
	}
}

pub(super) fn validate_social_post_claims(claims: Option<&Value>, errors: &mut Vec<String>) {
	let Some(claims) = claims.and_then(Value::as_array) else {
		return;
	};

	for (index, claim) in claims.iter().enumerate() {
		let Some(claim) = claim.as_object() else {
			errors.push(format!("claims[{index}] must be an object"));

			continue;
		};

		for field in ["text", "evidence"] {
			if !is_non_empty_string(claim.get(field)) {
				errors.push(format!("claims[{index}].{field} must be a non-empty string"));
			}
		}

		if !matches_one_of(claim.get("confidence"), SIGNAL_CONFIDENCE) {
			errors.push(format!(
				"claims[{index}].confidence must be one of {}",
				choices(SIGNAL_CONFIDENCE)
			));
		}
	}
}

pub(super) fn validate_social_post_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(decision) = entry.get("decision").and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};

	if !matches_one_of(decision.get("worthiness"), SOCIAL_POST_WORTHINESS) {
		errors.push(format!(
			"decision.worthiness must be one of {}",
			choices(SOCIAL_POST_WORTHINESS)
		));
	}
	if !matches_one_of(decision.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors
			.push(format!("decision.priority must be one of {}", choices(SOCIAL_POST_PRIORITIES)));
	}

	for field in ["idempotency_key", "reason", "day", "timezone"] {
		if !is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}

	if decision.get("daily_limit").and_then(Value::as_i64) != Some(8) {
		errors.push("decision.daily_limit must be 8".into());
	}

	validate_social_post_decision_counts(entry, decision, errors);
}

pub(super) fn validate_social_post_decision_counts(
	entry: &Map<String, Value>,
	decision: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["daily_count_before", "daily_count_after"] {
		if decision.get(field).and_then(Value::as_i64).is_none_or(|value| value < 0) {
			errors.push(format!("decision.{field} must be a non-negative integer"));
		}
	}

	let before = decision.get("daily_count_before").and_then(Value::as_i64);
	let after = decision.get("daily_count_after").and_then(Value::as_i64);
	let post_count = entry.get("text").and_then(Value::as_array).map_or(0, Vec::len) as i64;

	if let (Some(before), Some(after)) = (before, after) {
		if string_field(entry, "status") == Some("published") && after != before + post_count {
			errors.push("decision.daily_count_after must add the published post count".into());
		}
		if string_field(entry, "status") != Some("published") && after != before {
			errors.push("decision.daily_count_after must remain unchanged unless published".into());
		}
	}
}

pub(super) fn validate_social_post_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	match string_field(entry, "status") {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") if entry.get("failure").and_then(Value::as_object).is_none() =>
			errors.push("failure is required when status is failed".into()),
		Some("skipped") if entry.get("skip").and_then(Value::as_object).is_none() =>
			errors.push("skip is required when status is skipped".into()),
		_ => {},
	}
}

pub(super) fn validate_social_post_lifecycle(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(lifecycle) = entry.get("post_lifecycle") else {
		return;
	};
	let Some(lifecycle) = lifecycle.as_object() else {
		errors.push("post_lifecycle must be an object when present".into());

		return;
	};

	if !matches_one_of(lifecycle.get("current_state"), SOCIAL_POST_LIFECYCLE_STATES) {
		errors.push(format!(
			"post_lifecycle.current_state must be one of {}",
			choices(SOCIAL_POST_LIFECYCLE_STATES)
		));
	}
	if lifecycle.get("quote_eligible").and_then(Value::as_bool).is_none() {
		errors.push("post_lifecycle.quote_eligible must be boolean".into());
	}
	if !is_non_empty_string(lifecycle.get("reason")) {
		errors.push("post_lifecycle.reason must be a non-empty string".into());
	}
	if lifecycle
		.get("superseded_by_candidate")
		.is_some_and(|value| !is_non_empty_string(Some(value)))
	{
		errors.push("post_lifecycle.superseded_by_candidate must be non-empty when present".into());
	}

	let current_state = string_field(lifecycle, "current_state");
	let quote_eligible = lifecycle.get("quote_eligible").and_then(Value::as_bool);

	if quote_eligible == Some(true)
		&& (string_field(entry, "status") != Some("published") || current_state != Some("live"))
	{
		errors
			.push("post_lifecycle.quote_eligible can be true only for live published posts".into());
	}
	if current_state.is_some_and(|state| state.starts_with("superseded"))
		&& lifecycle.get("superseded_by_candidate").is_none()
	{
		errors.push(
			"post_lifecycle.superseded_by_candidate is required for superseded states".into(),
		);
	}
}

pub(super) fn validate_social_post_publication(
	publication: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication is required when status is published".into());

		return;
	};

	if !matches_one_of(publication.get("publisher"), &["chrome", "x_api"]) {
		errors.push("publication.publisher must be chrome or x_api".into());
	}
	if publication.get("account_verified").and_then(Value::as_bool) != Some(true) {
		errors.push("publication.account_verified must be true".into());
	}
	if publication.get("made_with_ai").and_then(Value::as_bool).is_none() {
		errors.push("publication.made_with_ai must be boolean".into());
	}
	if publication.get("image_template").is_some()
		&& string_field(publication, "image_template") != Some("decodex_signal_card")
	{
		errors.push("publication.image_template must be decodex_signal_card when present".into());
	}
	if !non_empty_array(publication.get("published_urls"))
		.is_some_and(|urls| urls.iter().all(|url| is_https_string(Some(url))))
	{
		errors.push("publication.published_urls must be a non-empty list of https URLs".into());
	}
	if !is_non_empty_string(publication.get("posted_at")) {
		errors.push("publication.posted_at must be a non-empty string".into());
	}
}

pub(super) fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(block) = entry.get("block").and_then(Value::as_object) else {
		errors.push("block is required when status is blocked".into());

		return;
	};

	if !matches_one_of(block.get("reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!("block.reason must be one of {}", choices(SOCIAL_BLOCK_REASONS)));
	}

	let count_before = entry
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("daily_count_before"))
		.and_then(Value::as_i64);

	if string_field(block, "reason") == Some("daily_cap_exceeded")
		&& count_before.is_none_or(|count| count < 8)
	{
		errors.push("daily_cap_exceeded requires decision.daily_count_before >= 8".into());
	}
	if !is_non_empty_string(block.get("operator_notice")) {
		errors.push("block.operator_notice must be a non-empty string".into());
	}
}
