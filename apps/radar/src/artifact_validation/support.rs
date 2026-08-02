//! Shared JSON field and scalar validation helpers.

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::artifact_validation::{
	BUNDLE_SCHEMA, CONFIG_FEATURE_CATALOG_SCHEMA, RELEASE_DELTA_SCHEMA, SIGNAL_SCHEMA,
	UPSTREAM_IMPACT_SCHEMA, UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA,
};

pub(super) fn validate_non_empty_string_list(
	value: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let valid = non_empty_array(value).is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	});

	if !valid {
		errors.push(format!("{label} must be a non-empty list of strings"));
	}
}

pub(super) fn validate_git_object_id(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = value.and_then(Value::as_str).is_some_and(is_git_object_id);

	if !valid {
		errors.push(format!("{label} must be a lowercase 40- or 64-character Git object id"));
	}
}

pub(super) fn validate_git_object_id_list(
	value: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let Some(values) = non_empty_array(value) else {
		errors.push(format!("{label} must be a non-empty list of Git object ids"));

		return;
	};
	let mut seen = std::collections::BTreeSet::new();

	for item in values {
		let Some(item) = item.as_str().filter(|item| is_git_object_id(item)) else {
			errors.push(format!(
				"{label} must contain only lowercase 40- or 64-character Git object ids"
			));

			return;
		};
		if !seen.insert(item) {
			errors.push(format!("{label} must not contain duplicate Git object ids"));

			return;
		}
	}
}

pub(super) fn validate_sha256(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = value.and_then(Value::as_str).is_some_and(|value| {
		value.len() == 64
			&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
	});

	if !valid {
		errors.push(format!("{label} must be a lowercase SHA-256 digest"));
	}
}

fn is_git_object_id(value: &str) -> bool {
	matches!(value.len(), 40 | 64)
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn validate_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = value.and_then(Value::as_array).is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	});

	if !valid {
		errors.push(format!("{label} must be a list"));
	}
}

pub(super) fn validate_optional_string_list(
	value: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let Some(value) = value else {
		return;
	};

	if value.is_null() {
		return;
	}
	if !value.as_array().is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	}) {
		errors.push(format!("{label} must be a list of non-empty strings when present"));
	}
}

pub(super) fn validate_rfc3339_field(
	entry: &Map<String, Value>,
	field: &str,
	errors: &mut Vec<String>,
) {
	let Some(value) = entry.get(field).and_then(Value::as_str).filter(|value| !value.is_empty())
	else {
		return;
	};

	if OffsetDateTime::parse(value, &Rfc3339).is_err() {
		errors.push(format!("{field} must be an RFC3339 timestamp"));
	}
}

pub(super) fn validate_optional_positive_integer_list(
	value: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let Some(value) = value else {
		return;
	};

	if value.is_null() {
		return;
	}
	if !value
		.as_array()
		.is_some_and(|values| values.iter().all(|item| item.as_i64().is_some_and(|item| item > 0)))
	{
		errors.push(format!("{label} must be a list of positive integers"));
	}
}

pub(super) fn optional_array<'a>(
	value: Option<&'a Value>,
	label: &str,
	errors: &mut Vec<String>,
) -> Option<&'a Vec<Value>> {
	match value {
		Some(Value::Array(values)) => Some(values),
		Some(Value::Null) | None => None,
		Some(_) => {
			errors.push(format!("{label} must be a list when present"));

			None
		},
	}
}

pub(super) fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

pub(super) fn is_non_empty_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| !value.is_empty())
}

pub(super) fn is_truthy_json_value(value: Option<&Value>) -> bool {
	match value {
		Some(Value::Null) | None => false,
		Some(Value::String(value)) => !value.is_empty(),
		Some(_) => true,
	}
}

pub(super) fn matches_one_of(value: Option<&Value>, choices: &[&str]) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| choices.contains(&value))
}

pub(super) fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	value.and_then(Value::as_array).filter(|values| !values.is_empty())
}

pub(super) fn is_empty_or_missing_array(value: Option<&Value>) -> bool {
	value.and_then(Value::as_array).is_none_or(Vec::is_empty)
}

pub(super) fn is_https_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| value.starts_with("https://"))
}

pub(super) fn is_https_string_array(value: &Value) -> bool {
	value.as_array().is_some_and(|values| values.iter().all(|url| is_https_string(Some(url))))
}

pub(super) fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}

pub(super) fn known_schemas() -> String {
	choices(&[
		BUNDLE_SCHEMA,
		CONFIG_FEATURE_CATALOG_SCHEMA,
		RELEASE_DELTA_SCHEMA,
		SIGNAL_SCHEMA,
		UPSTREAM_IMPACT_SCHEMA,
		UPSTREAM_REVIEW_QUEUE_SCHEMA,
		UPSTREAM_REVIEW_SCHEMA,
	])
}
