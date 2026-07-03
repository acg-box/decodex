use crate::{Map, Value, eyre, prelude::Result};

pub(super) fn object_field<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> Result<&'a Map<String, Value>> {
	object
		.get(field)
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

pub(super) fn required_u64(object: &Map<String, Value>, field: &str, label: &str) -> Result<u64> {
	object
		.get(field)
		.and_then(Value::as_u64)
		.ok_or_else(|| eyre::eyre!("{label} must be an unsigned integer"))
}

pub(super) fn required_i64(object: &Map<String, Value>, field: &str, label: &str) -> Result<i64> {
	object
		.get(field)
		.and_then(Value::as_i64)
		.ok_or_else(|| eyre::eyre!("{label} must be an integer"))
}

pub(super) fn pr_labels(pr: &Map<String, Value>) -> Vec<String> {
	pr.get("labels")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|label| {
			label
				.as_object()
				.and_then(|label| label.get("name"))
				.and_then(Value::as_str)
				.map(str::to_owned)
		})
		.collect()
}
