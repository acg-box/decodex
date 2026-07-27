//! social_strategy/v1 schema validation.

use std::collections::BTreeSet;

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
			"editorial_benchmark",
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
	if social_validation::string_field(entry, "cycle_key")
		.is_some_and(|value| value.chars().count() > 128)
	{
		errors.push("cycle_key must contain at most 128 characters".into());
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
	validate_evidence_refs(entry.get("evidence_refs"), errors);
	validate_decisions(entry.get("decisions"), errors);
	validate_editorial_benchmark(entry, errors);
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
		if social_validation::string_field(decision, "key")
			.is_some_and(|value| value.chars().count() > 128)
		{
			errors.push(format!("{label}.key must contain at most 128 characters"));
		}
		if social_validation::string_field(decision, "reason")
			.is_some_and(|value| value.chars().count() > 512)
		{
			errors.push(format!("{label}.reason must contain at most 512 characters"));
		}
		for field in ["previous_value", "next_value"] {
			if !decision.get(field).is_some_and(valid_strategy_value) {
				errors.push(format!("{label}.{field} must be a bounded string or number"));
			}
		}
	}
}

fn validate_evidence_refs(value: Option<&Value>, errors: &mut Vec<String>) {
	let Some(values) = social_validation::non_empty_array(value) else {
		errors.push("evidence_refs must be a non-empty list".into());

		return;
	};
	if values.len() > 64 {
		errors.push("evidence_refs must contain at most 64 entries".into());
	}
	let mut unique = BTreeSet::new();

	for (index, value) in values.iter().enumerate() {
		let Some(value) = value.as_str().filter(|value| !value.is_empty()) else {
			errors.push(format!("evidence_refs[{index}] must be a non-empty string"));

			continue;
		};
		if value.chars().count() > 512 {
			errors.push(format!("evidence_refs[{index}] must contain at most 512 characters"));
		}
		if !unique.insert(value) {
			errors.push(format!("evidence_refs[{index}] must be unique"));
		}
	}
}

fn validate_editorial_benchmark(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let weekly = social_validation::string_field(entry, "cadence") == Some("weekly");
	let benchmark_decision_count = weekly_benchmark_decision_count(entry);
	if !weekly && benchmark_decision_count != 0 {
		errors
			.push("weekly_editorial_benchmark decision must be absent for daily strategies".into());
	}
	let Some(benchmark) = entry.get("editorial_benchmark") else {
		if weekly {
			errors.push("editorial_benchmark is required for weekly strategies".into());
		}

		return;
	};
	if !weekly {
		errors.push("editorial_benchmark must be absent for daily strategies".into());

		return;
	}
	let Some(benchmark) = benchmark.as_object() else {
		errors.push("editorial_benchmark must be an object".into());

		return;
	};
	social_validation::validate_exact_keys(
		benchmark,
		"editorial_benchmark",
		&["observations", "public_post_urls", "reason_code", "status"],
		errors,
	);
	let status = social_validation::string_field(benchmark, "status");
	if !matches!(status, Some("completed" | "deferred")) {
		errors.push("editorial_benchmark.status must be completed or deferred".into());
	}
	validate_bounded_string_list(
		benchmark.get("observations"),
		"editorial_benchmark.observations",
		12,
		280,
		false,
		errors,
	);

	let evidence = entry
		.get("evidence_refs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.collect::<BTreeSet<_>>();
	match status {
		Some("completed") => {
			if benchmark.get("reason_code").is_some() {
				errors.push("editorial_benchmark.reason_code must be absent when completed".into());
			}
			let Some(urls) = benchmark.get("public_post_urls").and_then(Value::as_array) else {
				errors
					.push("editorial_benchmark.public_post_urls is required when completed".into());

				validate_weekly_benchmark_decision(entry, errors);

				return;
			};
			validate_bounded_string_list(
				benchmark.get("public_post_urls"),
				"editorial_benchmark.public_post_urls",
				12,
				160,
				true,
				errors,
			);
			for (index, url) in urls.iter().enumerate() {
				let Some(url) = url.as_str() else {
					continue;
				};
				if !valid_benchmark_url(url) {
					errors.push(format!(
						"editorial_benchmark.public_post_urls[{index}] must be a supported public X status URL"
					));
				}
				if !evidence.contains(url) {
					errors.push(format!(
						"editorial_benchmark.public_post_urls[{index}] must appear in evidence_refs"
					));
				}
			}
		},
		Some("deferred") => {
			if benchmark.get("public_post_urls").is_some() {
				errors.push(
					"editorial_benchmark.public_post_urls must be absent when deferred".into(),
				);
			}
			let reason = social_validation::string_field(benchmark, "reason_code");
			if !reason.is_some_and(valid_reason_code) {
				errors.push(
					"editorial_benchmark.reason_code must be a bounded reason code when deferred"
						.into(),
				);
			} else if let Some(reason) = reason {
				let marker = format!("benchmark:deferred:{reason}");
				if !evidence.contains(marker.as_str()) {
					errors.push(
						"deferred editorial benchmark reason must appear in evidence_refs".into(),
					);
				}
			}
		},
		_ => {},
	}
	validate_weekly_benchmark_decision(entry, errors);
}

fn validate_weekly_benchmark_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let count = weekly_benchmark_decision_count(entry);
	if count != 1 {
		errors.push(
			"weekly strategies must contain exactly one weekly_editorial_benchmark decision".into(),
		);
	}
}

fn weekly_benchmark_decision_count(entry: &Map<String, Value>) -> usize {
	entry.get("decisions").and_then(Value::as_array).map_or(0, |decisions| {
		decisions
			.iter()
			.filter(|decision| {
				decision.get("key").and_then(Value::as_str) == Some("weekly_editorial_benchmark")
			})
			.count()
	})
}

fn validate_bounded_string_list(
	value: Option<&Value>,
	label: &str,
	maximum_items: usize,
	maximum_characters: usize,
	require_unique: bool,
	errors: &mut Vec<String>,
) {
	let Some(values) = social_validation::non_empty_array(value) else {
		errors.push(format!("{label} must be a non-empty list"));

		return;
	};
	if values.len() > maximum_items {
		errors.push(format!("{label} must contain at most {maximum_items} entries"));
	}
	let mut unique = BTreeSet::new();

	for (index, value) in values.iter().enumerate() {
		let Some(value) = value.as_str().filter(|value| !value.is_empty()) else {
			errors.push(format!("{label}[{index}] must be a non-empty string"));

			continue;
		};
		if value.chars().count() > maximum_characters {
			errors.push(format!(
				"{label}[{index}] must contain at most {maximum_characters} characters"
			));
		}
		if require_unique && !unique.insert(value) {
			errors.push(format!("{label}[{index}] must be unique"));
		}
	}
}

fn valid_benchmark_url(value: &str) -> bool {
	let Some(path) = value.strip_prefix("https://x.com/") else {
		return false;
	};
	let parts = path.split('/').collect::<Vec<_>>();

	parts.len() == 3
		&& matches!(parts[0], "CodexReleases" | "Codex_Changelog" | "decodexspace")
		&& parts[1] == "status"
		&& !parts[2].is_empty()
		&& parts[2].bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_reason_code(value: &str) -> bool {
	let bytes = value.as_bytes();
	!bytes.is_empty()
		&& bytes.len() <= 64
		&& bytes[0].is_ascii_lowercase()
		&& bytes
			.iter()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
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
