use serde_json::Value;

pub(super) fn string_like_json_value(value: &Value) -> Option<String> {
	match value {
		Value::String(text) if !text.is_empty() => Some(text.clone()),
		Value::Number(number) => Some(number.to_string()),
		Value::Bool(value) => Some(value.to_string()),
		Value::Object(entries) =>
			["message", "text", "id", "codexErrorInfo", "type", "kind", "code", "reason", "name"]
				.iter()
				.find_map(|key| entries.get(*key).and_then(string_like_json_value))
				.or_else(|| {
					(entries.len() == 1)
						.then(|| entries.values().next().and_then(string_like_json_value))
						.flatten()
				}),
		Value::Array(items) => items.iter().find_map(string_like_json_value),
		_ => None,
	}
}

pub(super) fn externally_tagged_value_name(value: &Value) -> Option<String> {
	match value {
		Value::String(value) if !value.is_empty() => Some(value.clone()),
		Value::Object(object) => object
			.get("type")
			.and_then(Value::as_str)
			.map(str::to_owned)
			.or_else(|| (object.len() == 1).then(|| object.keys().next().cloned()).flatten()),
		_ => None,
	}
}
