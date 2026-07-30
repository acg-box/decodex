//! social_publish_reservation/v1 schema validation.

use crate::social_validation::{
	self, Map, SOCIAL_POST_MODES, SOCIAL_PUBLISH_RESERVATION_STATUSES, Value,
};

pub(super) fn validate_social_publish_reservation(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	social_validation::validate_exact_keys(
		entry,
		"social_publish_reservation",
		&[
			"candidate_refs",
			"channel",
			"consumed_by_social_post",
			"day",
			"duplicate_keys",
			"evidence_notes",
			"expires_at",
			"idempotency_key",
			"mode",
			"owner",
			"publication_lineage_sha256",
			"release_reason",
			"reserved_at",
			"schema",
			"slug",
			"status",
			"target_account",
			"timezone",
		],
		errors,
	);

	for field in ["slug", "idempotency_key", "reserved_at", "expires_at", "day", "timezone"] {
		if !social_validation::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}
	if !entry.get("publication_lineage_sha256").and_then(Value::as_str).is_some_and(valid_sha256) {
		errors.push("publication_lineage_sha256 must be a lowercase SHA-256 digest".into());
	}

	validate_social_publish_reservation_constants(entry, errors);
	validate_social_publish_reservation_refs(entry.get("candidate_refs"), errors);

	social_validation::validate_non_empty_string_list(
		entry.get("duplicate_keys"),
		"duplicate_keys",
		errors,
	);
	if entry.get("duplicate_keys").and_then(Value::as_array).map(Vec::len) != Some(2) {
		errors.push(
			"duplicate_keys must contain exactly the candidate slug and idempotency_key".into(),
		);
	}
	social_validation::validate_optional_string_list(
		entry.get("evidence_notes"),
		"evidence_notes",
		errors,
	);

	validate_social_publish_reservation_owner(entry.get("owner"), errors);

	social_validation::validate_rfc3339_field(entry, "reserved_at", errors);
	social_validation::validate_rfc3339_field(entry, "expires_at", errors);

	validate_social_publish_reservation_status_payload(entry, errors);
}

fn valid_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_social_publish_reservation_constants(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
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
	if !social_validation::matches_one_of(entry.get("status"), SOCIAL_PUBLISH_RESERVATION_STATUSES)
	{
		errors.push(format!(
			"status must be one of {}",
			social_validation::choices(SOCIAL_PUBLISH_RESERVATION_STATUSES)
		));
	}
}

fn validate_social_publish_reservation_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("candidate_refs must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(refs, "candidate_refs", &["social_candidates"], errors);
	social_validation::validate_optional_string_list(
		refs.get("social_candidates"),
		"candidate_refs.social_candidates",
		errors,
	);
	if refs.get("social_candidates").and_then(Value::as_array).map(Vec::len) != Some(1) {
		errors.push("candidate_refs.social_candidates must contain exactly one item".into());
	}
}

fn validate_social_publish_reservation_owner(owner: Option<&Value>, errors: &mut Vec<String>) {
	let Some(owner) = owner else {
		errors.push("owner is required".into());
		return;
	};
	let Some(owner) = owner.as_object() else {
		errors.push("owner must be an object when present".into());

		return;
	};
	social_validation::validate_exact_keys(owner, "owner", &["automation_id", "run_id"], errors);

	if social_validation::string_field(owner, "automation_id") != Some("decodex-xurl-publisher") {
		errors.push("owner.automation_id must be decodex-xurl-publisher".into());
	}
	let run_id = social_validation::string_field(owner, "run_id");
	if run_id.is_none_or(|value| !crate::social_publish::valid_run_id(value)) {
		errors.push("owner.run_id must be a lowercase UUID".into());
	}
}

fn validate_social_publish_reservation_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	match social_validation::string_field(entry, "status") {
		Some("active") =>
			for field in ["consumed_by_social_post", "release_reason"] {
				if entry.get(field).is_some() {
					errors.push(format!("{field} must be absent when status is active"));
				}
			},
		Some("consumed") => {
			if !social_validation::is_non_empty_string(entry.get("consumed_by_social_post")) {
				errors.push("consumed_by_social_post is required when status is consumed".into());
			}
			if entry.get("release_reason").is_some() {
				errors.push("release_reason must be absent when status is consumed".into());
			}
		},
		Some("canceled" | "expired") => {
			if !social_validation::is_non_empty_string(entry.get("release_reason")) {
				errors.push("release_reason is required when status is canceled or expired".into());
			}
			if entry.get("consumed_by_social_post").is_some() {
				errors.push(
					"consumed_by_social_post must be absent when status is canceled or expired"
						.into(),
				);
			}
		},
		_ => {},
	}
}
