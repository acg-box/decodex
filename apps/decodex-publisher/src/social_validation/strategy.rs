//! social_strategy/v1 schema validation.

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::social_validation::{self, Map, Value};

const STRATEGY_CADENCES: &[&str] = &["daily", "weekly"];
const STRATEGY_DIMENSIONS: &[&str] =
	&["format_preference", "no_change", "quality_threshold", "topic_weight"];
const STRATEGY_GUARDRAILS: &[&str] =
	&["account_gate", "evidence_gate", "idempotency_gate", "privacy_gate", "publication_gate"];

pub(super) fn validate_social_strategy(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	social_validation::validate_exact_keys(
		entry,
		"social_strategy",
		&[
			"cadence",
			"cycle_key",
			"decisions",
			"evidence_refs",
			"guardrails",
			"next_review_at",
			"reviewed_at",
			"schema",
		],
		errors,
	);

	for field in ["cycle_key", "reviewed_at", "next_review_at"] {
		if !social_validation::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}
	if !social_validation::matches_one_of(entry.get("cadence"), STRATEGY_CADENCES) {
		errors.push(format!(
			"cadence must be one of {}",
			social_validation::choices(STRATEGY_CADENCES)
		));
	}
	social_validation::validate_rfc3339_field(entry, "reviewed_at", errors);
	social_validation::validate_rfc3339_field(entry, "next_review_at", errors);
	validate_review_order(entry, errors);
	social_validation::validate_non_empty_string_list(
		entry.get("evidence_refs"),
		"evidence_refs",
		errors,
	);
	validate_decisions(entry.get("decisions"), errors);
	validate_numerical_change_evidence_count(entry, errors);
	validate_guardrails(entry.get("guardrails"), errors);
}

fn validate_review_order(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let (Some(reviewed_at), Some(next_review_at)) = (
		entry.get("reviewed_at").and_then(Value::as_str),
		entry.get("next_review_at").and_then(Value::as_str),
	) else {
		return;
	};
	let (Ok(reviewed_at), Ok(next_review_at)) = (
		OffsetDateTime::parse(reviewed_at, &Rfc3339),
		OffsetDateTime::parse(next_review_at, &Rfc3339),
	) else {
		return;
	};

	if next_review_at <= reviewed_at {
		errors.push("next_review_at must be later than reviewed_at".into());
	}
}

fn validate_decisions(decisions: Option<&Value>, errors: &mut Vec<String>) {
	let Some(decisions) = social_validation::non_empty_array(decisions) else {
		errors.push("decisions must be a non-empty list".into());

		return;
	};

	if decisions.len() > 16 {
		errors.push("decisions must contain at most 16 entries".into());
	}
	for (index, decision) in decisions.iter().enumerate() {
		let Some(decision) = decision.as_object() else {
			errors.push(format!("decisions[{index}] must be an object"));

			continue;
		};
		let label = format!("decisions[{index}]");
		social_validation::validate_exact_keys(
			decision,
			&label,
			&["dimension", "key", "next_value", "previous_value", "reason"],
			errors,
		);

		if !social_validation::matches_one_of(decision.get("dimension"), STRATEGY_DIMENSIONS) {
			errors.push(format!(
				"{label}.dimension must be one of {}",
				social_validation::choices(STRATEGY_DIMENSIONS)
			));
		}
		for field in ["key", "reason"] {
			if !social_validation::is_non_empty_string(decision.get(field)) {
				errors.push(format!("{label}.{field} must be a non-empty string"));
			}
		}
		for field in ["previous_value", "next_value"] {
			if !decision.get(field).is_some_and(valid_strategy_value) {
				errors.push(format!("{label}.{field} must be a bounded string or number"));
			}
		}
	}
}

fn validate_numerical_change_evidence_count(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let requires_three_outcomes =
		entry.get("decisions").and_then(Value::as_array).is_some_and(|decisions| {
			decisions.iter().any(|decision| {
				let Some(decision) = decision.as_object() else {
					return false;
				};
				let dimension = decision.get("dimension").and_then(Value::as_str);
				let previous = decision.get("previous_value").and_then(Value::as_f64);
				let next = decision.get("next_value").and_then(Value::as_f64);

				matches!(dimension, Some("topic_weight" | "format_preference"))
					&& previous.zip(next).is_some_and(|(previous, next)| previous != next)
			})
		});
	let evidence_count = entry.get("evidence_refs").and_then(Value::as_array).map_or(0, Vec::len);

	if requires_three_outcomes && evidence_count < 3 {
		errors.push(
			"numerical topic_weight or format_preference change requires at least three evidence_refs"
				.into(),
		);
	}
}

fn valid_strategy_value(value: &Value) -> bool {
	value.as_str().is_some_and(|value| !value.is_empty() && value.chars().count() <= 128)
		|| value
			.as_f64()
			.is_some_and(|value| value.is_finite() && (-100.0..=100.0).contains(&value))
}

fn validate_guardrails(guardrails: Option<&Value>, errors: &mut Vec<String>) {
	let Some(guardrails) = guardrails.and_then(Value::as_object) else {
		errors.push("guardrails must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(guardrails, "guardrails", STRATEGY_GUARDRAILS, errors);

	for field in STRATEGY_GUARDRAILS {
		if social_validation::string_field(guardrails, field) != Some("unchanged") {
			errors.push(format!("guardrails.{field} must be unchanged"));
		}
	}
}
