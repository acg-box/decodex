//! social_outcome/v1 schema validation.

use crate::social_validation::{self, Map, Value};

const OUTCOME_WINDOWS: &[&str] = &["24h", "7d"];
const METRIC_FIELDS: &[&str] = &["bookmarks", "likes", "replies", "reposts", "views"];

pub(super) fn validate_social_outcome(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_outcome",
		&[
			"metrics",
			"notes",
			"observation",
			"observed_at",
			"owner",
			"published_url",
			"schema",
			"slug",
			"social_post_ref",
			"target_account",
			"window",
		],
		errors,
	);

	for field in ["slug", "social_post_ref"] {
		if !social_validation::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if social_validation::string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if !entry
		.get("published_url")
		.and_then(Value::as_str)
		.is_some_and(|url| url.starts_with("https://x.com/decodexspace/status/"))
	{
		errors.push("published_url must be a decodexspace X status URL".into());
	}

	social_validation::validate_rfc3339_field(entry, "observed_at", errors);
	if !social_validation::is_non_empty_string(entry.get("observed_at")) {
		errors.push("observed_at must be a non-empty RFC3339 timestamp".into());
	}
	if !social_validation::matches_one_of(entry.get("window"), OUTCOME_WINDOWS) {
		errors
			.push(format!("window must be one of {}", social_validation::choices(OUTCOME_WINDOWS)));
	}

	validate_metrics(entry.get("metrics"), errors);
	validate_observation(entry.get("observation"), errors);
	validate_owner(entry.get("owner"), errors);
	social_validation::validate_optional_string_list(entry.get("notes"), "notes", errors);
}

fn validate_owner(value: Option<&Value>, errors: &mut Vec<String>) {
	let Some(owner) = value.and_then(Value::as_object) else {
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

fn validate_observation(value: Option<&Value>, errors: &mut Vec<String>) {
	let Some(observation) = value.and_then(Value::as_object) else {
		errors.push("observation must be an object".into());
		return;
	};
	social_validation::validate_exact_keys(
		observation,
		"observation",
		&[
			"publication_lineage_sha256",
			"reader",
			"recorded_cost_ceiling_microusd",
			"response_sha256",
			"verified_account",
			"xurl_app",
			"xurl_version",
		],
		errors,
	);
	if social_validation::string_field(observation, "reader") != Some("xurl") {
		errors.push("observation.reader must be xurl".into());
	}
	if social_validation::string_field(observation, "xurl_app") != Some("default") {
		errors.push("observation.xurl_app must be default".into());
	}
	if social_validation::string_field(observation, "verified_account") != Some("decodexspace") {
		errors.push("observation.verified_account must be decodexspace".into());
	}
	if !observation.get("publication_lineage_sha256").and_then(Value::as_str).is_some_and(|value| {
		value.len() == 64
			&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
	}) {
		errors.push(
			"observation.publication_lineage_sha256 must be a lowercase SHA-256 digest".into(),
		);
	}
	if observation.get("recorded_cost_ceiling_microusd").and_then(Value::as_u64) != Some(5_000) {
		errors.push("observation.recorded_cost_ceiling_microusd must be 5000".into());
	}
	if social_validation::string_field(observation, "xurl_version") != Some("1.3.1") {
		errors.push("observation.xurl_version must be exactly 1.3.1".into());
	}
	if !observation.get("response_sha256").and_then(Value::as_str).is_some_and(|value| {
		value.len() == 64
			&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
	}) {
		errors.push("observation.response_sha256 must be a lowercase SHA-256 digest".into());
	}
}

fn validate_metrics(metrics: Option<&Value>, errors: &mut Vec<String>) {
	let Some(metrics) = metrics.and_then(Value::as_object) else {
		errors.push("metrics must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(metrics, "metrics", METRIC_FIELDS, errors);

	if metrics.is_empty() {
		errors.push("metrics must include at least one supported metric".into());
	}

	for (field, value) in metrics {
		if !METRIC_FIELDS.contains(&field.as_str()) {
			errors.push(format!("metrics.{field} is not supported"));

			continue;
		}
		if !value.as_u64().is_some() {
			errors.push(format!("metrics.{field} must be a non-negative integer"));
		}
	}
}
