//! Shared social artifact validation predicates.

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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

pub(super) fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

pub(super) fn is_non_empty_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| !value.is_empty())
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
