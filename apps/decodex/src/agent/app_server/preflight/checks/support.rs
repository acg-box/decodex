use crate::agent::app_server::preflight::{BTreeMap, ModelSummary, Value};

pub(crate) fn model_matches_config(model: &ModelSummary, configured_model: &str) -> bool {
	model.model == configured_model || model.id == configured_model
}

pub(crate) fn insert_optional_detail(
	details: &mut BTreeMap<String, String>,
	name: &str,
	value: Option<&str>,
) {
	if let Some(value) = value.filter(|value| !value.is_empty()) {
		details.insert(name.to_owned(), value.to_owned());
	}
}

pub(crate) fn config_value_name(value: &Value) -> Option<String> {
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
