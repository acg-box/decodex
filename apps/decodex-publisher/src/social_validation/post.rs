//! social_post/v1 schema validation.

#[allow(clippy::wildcard_imports)] use super::*;

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

fn validate_social_post_constants(entry: &Map<String, Value>, errors: &mut Vec<String>) {
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

	for (index, text) in items.iter().enumerate() {
		let Some(text) = text.as_str() else {
			errors.push(format!("text[{index}] must be a string"));

			continue;
		};

		validate_social_post_text_item(text, index, errors);
	}
}

fn validate_social_post_text_item(text: &str, index: usize, errors: &mut Vec<String>) {
	if text.is_empty() || text.chars().count() > 280 {
		errors.push(format!("text[{index}] must be a non-empty X-sized string"));
	}
	if text.contains("Automated by @hackink") {
		errors.push(format!("text[{index}] must not include automation attribution"));
	}
	if text.chars().count() > 260 && !text.contains("https://") {
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

fn validate_social_post_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
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
	.any(|field| refs.get(*field).is_some_and(|value| !is_empty_or_missing_array(Some(value))));

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
	let Some(claims) = non_empty_array(claims) else {
		errors.push("claims must be a non-empty list of claim objects".into());

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

fn validate_social_post_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
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

	validate_social_post_decision_counts(entry, decision, errors);
}

fn validate_social_post_decision_counts(
	entry: &Map<String, Value>,
	decision: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	if decision.get("daily_limit").and_then(Value::as_i64) != Some(8) {
		errors.push("decision.daily_limit must be 8".into());
	}

	let before = decision.get("daily_count_before").and_then(Value::as_i64);
	let after = decision.get("daily_count_after").and_then(Value::as_i64);

	match string_field(entry, "status") {
		Some("published")
			if before.zip(after).is_none_or(|(before, after)| after != before + 1) =>
			errors.push(
				"decision.daily_count_after must equal daily_count_before + 1 for published posts"
					.into(),
			),
		Some("blocked" | "failed" | "skipped")
			if before.zip(after).is_none_or(|(before, after)| after != before) =>
			errors.push(
				"decision.daily_count_after must equal daily_count_before for non-published posts"
					.into(),
			),
		_ => {},
	}
}

fn validate_social_post_status_payload(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	match string_field(entry, "status") {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") if !is_non_empty_string(entry.get("failure_reason")) =>
			errors.push("failure_reason is required when status is failed".into()),
		Some("skipped") if !is_non_empty_string(entry.get("skip_reason")) =>
			errors.push("skip_reason is required when status is skipped".into()),
		_ => {},
	}
}

fn validate_social_post_lifecycle(entry: &Map<String, Value>, errors: &mut Vec<String>) {
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
	if !lifecycle.get("quote_eligible").is_some_and(Value::is_boolean) {
		errors.push("post_lifecycle.quote_eligible must be a boolean".into());
	}
	if lifecycle.get("reason").is_some_and(|value| !is_non_empty_string(Some(value))) {
		errors.push("post_lifecycle.reason must be non-empty when present".into());
	}
	if lifecycle
		.get("superseded_by_candidate")
		.is_some_and(|value| !is_non_empty_string(Some(value)))
	{
		errors.push("post_lifecycle.superseded_by_candidate must be non-empty when present".into());
	}
	if lifecycle.get("current_state").and_then(Value::as_str) != Some("live")
		&& lifecycle.get("quote_eligible").and_then(Value::as_bool) == Some(true)
	{
		errors
			.push("post_lifecycle.quote_eligible can be true only for live published posts".into());
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication must be an object when status is published".into());

		return;
	};

	for field in ["posted_at", "publisher"] {
		if !is_non_empty_string(publication.get(field)) {
			errors.push(format!("publication.{field} must be a non-empty string"));
		}
	}
	validate_rfc3339_field(publication, "posted_at", errors);
	if !publication.get("account_verified").is_some_and(Value::is_boolean) {
		errors.push("publication.account_verified must be a boolean".into());
	}
	if !publication.get("made_with_ai").is_some_and(Value::is_boolean) {
		errors.push("publication.made_with_ai must be a boolean".into());
	}
	if publication
		.get("published_urls")
		.is_some_and(|urls| !is_https_string_array(urls) || is_empty_or_missing_array(Some(urls)))
	{
		errors.push("publication.published_urls must be a non-empty list of https URLs".into());
	}
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if !matches_one_of(entry.get("block_reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!("block_reason must be one of {}", choices(SOCIAL_BLOCK_REASONS)));
	}
}
