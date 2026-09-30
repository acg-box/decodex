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

	let before = decision.get("daily_count_before").and_then(Value::as_u64);
	let after = decision.get("daily_count_after").and_then(Value::as_u64);

	match social_validation::string_field(entry, "status") {
		Some("published")
			if before
				.zip(after)
				.is_none_or(|(before, after)| before.checked_add(1) != Some(after)) =>
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

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};

	fn errors(status: &str, before: Value, after: Value) -> Vec<String> {
		let entry = json!({"status": status});
		let decision =
			json!({"daily_limit": 1, "daily_count_before": before, "daily_count_after": after});
		let mut errors = Vec::new();
		super::validate_social_post_decision_counts(
			entry.as_object().unwrap(),
			decision.as_object().unwrap(),
			&mut errors,
		);
		errors
	}

	#[test]
	fn decision_counts_reject_negative_and_overflowing_values() {
		assert!(!errors("published", json!(i64::MAX), json!(0)).is_empty());
		assert!(!errors("published", json!(u64::MAX), json!(0)).is_empty());
		assert!(!errors("published", json!(-1), json!(0)).is_empty());
		for status in ["blocked", "failed", "skipped"] {
			assert!(!errors(status, json!(-1), json!(-1)).is_empty());
		}
	}

	#[test]
	fn decision_counts_preserve_valid_state_transitions() {
		assert!(errors("published", json!(0), json!(1)).is_empty());
		assert!(!errors("published", json!(0), json!(0)).is_empty());
		for status in ["blocked", "failed", "skipped"] {
			assert!(errors(status, json!(0), json!(0)).is_empty());
			assert!(errors(status, json!(1), json!(1)).is_empty());
			assert!(!errors(status, json!(0), json!(1)).is_empty());
		}
	}
}
