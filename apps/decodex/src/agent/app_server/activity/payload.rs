use serde_json::{self, Value};

pub(super) fn extract_tool_name(value: &Value) -> Option<String> {
	let tool = string_at_paths(
		value,
		&[
			&["params", "tool"],
			&["params", "name"],
			&["params", "item", "tool"],
			&["params", "item", "name"],
			&["tool"],
			&["name"],
			&["item", "tool"],
			&["item", "name"],
		],
	)?;
	let namespace = string_at_paths(value, &[&["params", "namespace"], &["namespace"]]);

	Some(match namespace {
		Some(namespace) if !namespace.is_empty() => format!("{namespace}.{tool}"),
		_ => tool,
	})
}

pub(super) fn extract_tool_arguments(value: &Value) -> Option<Value> {
	let arguments = value_at_paths(
		value,
		&[
			&["params", "arguments"],
			&["params", "item", "arguments"],
			&["arguments"],
			&["item", "arguments"],
		],
	)?;

	if let Some(arguments_text) = arguments.as_str()
		&& let Ok(parsed_arguments) = serde_json::from_str::<Value>(arguments_text)
	{
		return Some(parsed_arguments);
	}

	Some(arguments.clone())
}

pub(super) fn extract_command_text(arguments: &Value) -> Option<String> {
	string_at_paths(arguments, &[&["cmd"], &["command"], &["argv", "0"]])
}

pub(super) fn string_at_paths(value: &Value, paths: &[&[&str]]) -> Option<String> {
	paths
		.iter()
		.find_map(|path| value_at_path(value, path).and_then(Value::as_str).map(str::to_owned))
}

pub(super) fn value_at_paths<'a>(value: &'a Value, paths: &[&[&str]]) -> Option<&'a Value> {
	paths.iter().find_map(|path| value_at_path(value, path))
}

pub(super) fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
	let mut current = value;

	for part in path {
		current = current.get(*part)?;
	}

	Some(current)
}

pub(super) fn tool_output_size(value: Option<&Value>, payload: &str) -> i64 {
	let largest_string = value.map(largest_string_len).unwrap_or(0);
	let payload_len = i64::try_from(payload.len()).unwrap_or(i64::MAX);

	largest_string.max(payload_len)
}

pub(super) fn largest_string_len(value: &Value) -> i64 {
	match value {
		Value::String(text) => i64::try_from(text.len()).unwrap_or(i64::MAX),
		Value::Array(items) => items.iter().map(largest_string_len).max().unwrap_or(0),
		Value::Object(entries) => entries.values().map(largest_string_len).max().unwrap_or(0),
		_ => 0,
	}
}

pub(super) fn find_numeric_field(value: &Value, keys: &[&str]) -> Option<i64> {
	match value {
		Value::Object(entries) => {
			for (key, nested) in entries {
				if keys.iter().any(|candidate| *candidate == key)
					&& let Some(number) = json_number_to_i64(nested)
				{
					return Some(number);
				}
			}

			entries.values().find_map(|nested| find_numeric_field(nested, keys))
		},
		Value::Array(items) => items.iter().find_map(|nested| find_numeric_field(nested, keys)),
		_ => None,
	}
}

pub(super) fn find_string_field(value: &Value, keys: &[&str]) -> Option<String> {
	match value {
		Value::Object(entries) => {
			for (key, nested) in entries {
				if keys.iter().any(|candidate| *candidate == key)
					&& let Some(text) = string_like_json_value(nested)
				{
					return Some(text);
				}
			}

			entries.values().find_map(|nested| find_string_field(nested, keys))
		},
		Value::Array(items) => items.iter().find_map(|nested| find_string_field(nested, keys)),
		_ => None,
	}
}

pub(super) fn string_like_json_value(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		Value::Object(entries) => ["kind", "type"]
			.iter()
			.find_map(|key| entries.get(*key).and_then(string_like_json_value)),
		_ => None,
	}
}

pub(super) fn json_number_to_i64(value: &Value) -> Option<i64> {
	value.as_i64().or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

pub(in crate::agent::app_server) fn redact_identifier(identifier: &str) -> String {
	let tail =
		identifier.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<String>();

	if tail.is_empty() { String::from("unknown") } else { format!("...{tail}") }
}
