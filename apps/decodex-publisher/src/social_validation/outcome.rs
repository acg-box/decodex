//! social_outcome/v1 schema validation.

use crate::social_validation::{self, Map, Value, post::BrowserSessionRequirement};

const OUTCOME_WINDOWS: &[&str] = &["24h", "7d"];
const METRIC_FIELDS: &[&str] = &["bookmarks", "likes", "replies", "reposts", "views"];

pub(super) fn validate_social_outcome(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_outcome",
		&[
			"browser_session",
			"metrics",
			"notes",
			"observed_at",
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
	super::post::validate_browser_session(
		entry.get("browser_session"),
		BrowserSessionRequirement::Complete,
		errors,
	);
	social_validation::validate_optional_string_list(entry.get("notes"), "notes", errors);
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
