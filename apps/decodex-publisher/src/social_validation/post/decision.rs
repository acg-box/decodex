use crate::social_validation::{self, Map, SOCIAL_POST_PRIORITIES, SOCIAL_POST_WORTHINESS, Value};

pub(super) fn validate_social_post_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(decision) = entry.get("decision").and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		decision,
		"decision",
		&[
			"daily_count_after",
			"daily_count_before",
			"daily_limit",
			"day",
			"idempotency_key",
			"priority",
			"reason",
			"timezone",
			"worthiness",
		],
		errors,
	);

	if !social_validation::matches_one_of(decision.get("worthiness"), SOCIAL_POST_WORTHINESS) {
		errors.push(format!(
			"decision.worthiness must be one of {}",
			social_validation::choices(SOCIAL_POST_WORTHINESS)
		));
	}
	if !social_validation::matches_one_of(decision.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors.push(format!(
			"decision.priority must be one of {}",
			social_validation::choices(SOCIAL_POST_PRIORITIES)
		));
	}

	for field in ["idempotency_key", "reason", "day", "timezone"] {
		if !social_validation::is_non_empty_string(decision.get(field)) {
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
	if decision.get("daily_limit").and_then(Value::as_i64) != Some(1) {
		errors.push("decision.daily_limit must be 1".into());
	}

	let before = decision.get("daily_count_before").and_then(Value::as_i64);
	let after = decision.get("daily_count_after").and_then(Value::as_i64);

	match social_validation::string_field(entry, "status") {
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
