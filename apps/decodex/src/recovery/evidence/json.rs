pub(super) fn bool_is_false(value: Option<&serde_json::Value>) -> bool {
	value.and_then(serde_json::Value::as_bool) == Some(false)
}

pub(super) fn number_is_zero_or_missing(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_u64() == Some(0))
}

pub(super) fn string_is_missing_or_empty(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_str().is_none_or(|value| value.is_empty()) || value.is_null())
}

pub(super) fn array_is_missing_or_empty(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_array().is_none_or(Vec::is_empty))
}
