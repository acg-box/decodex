//! social_publish_reservation/v1 schema validation.

#[allow(clippy::wildcard_imports)] use super::*;

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

fn validate_social_publish_reservation_constants(
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

fn validate_social_publish_reservation_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
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

fn validate_social_publish_reservation_owner(owner: Option<&Value>, errors: &mut Vec<String>) {
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

fn validate_social_publish_reservation_status_payload(
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
