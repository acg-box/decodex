use crate::{Map, OffsetDateTime, Rfc3339, Value, eyre, prelude::Result};

pub(crate) fn require_member(value: &str, allowed: &[&str], label: &str) -> Result<()> {
	if allowed.contains(&value) {
		Ok(())
	} else {
		eyre::bail!("{label} must be one of {}", choices(allowed))
	}
}

pub(crate) fn utc_now_iso() -> Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

pub(crate) fn object_value<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

pub(crate) fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

pub(crate) fn required_string<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> Result<&'a str> {
	string_field(object, field)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} must be a non-empty string"))
}

pub(crate) fn optional_string<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

pub(crate) fn is_truthy_json_value(value: Option<&Value>) -> bool {
	match value {
		Some(Value::Null) | None => false,
		Some(Value::String(value)) => !value.is_empty(),
		Some(_) => true,
	}
}

pub(crate) fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	value.and_then(Value::as_array).filter(|values| !values.is_empty())
}

pub(crate) fn first_line(value: &str) -> String {
	value.trim().lines().next().unwrap_or("").into()
}

fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}
