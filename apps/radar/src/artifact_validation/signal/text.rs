use serde_json::{Map, Value};

pub(crate) fn validate_multi_agent_v2_reference_text(
	entry: &Map<String, Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let mut text = String::new();

	collect_json_strings_from_map(entry, &mut text);

	let lower = text.to_ascii_lowercase();
	let mentions_v2 = lower.contains("multiagentv2")
		|| lower.contains("multi_agent_v2")
		|| lower.contains("multi-agent v2");

	if !mentions_v2 || !lower.contains("assign_task") {
		return;
	}
	if !lower.contains("followup_task") {
		errors.push(format!(
			"{label} that mention MultiAgentV2 assign_task must also mention current followup_task"
		));
	}
	if !has_legacy_multi_agent_v2_context(&lower) {
		errors.push(format!(
			"{label} must describe assign_task as legacy, historical, older, previous, or renamed context"
		));
	}
}

pub(crate) fn has_legacy_multi_agent_v2_context(text: &str) -> bool {
	["legacy", "historical", "older", "previous", "renamed", "rename"]
		.into_iter()
		.any(|term| text.contains(term))
}

fn collect_json_strings_from_map(object: &Map<String, Value>, text: &mut String) {
	for value in object.values() {
		collect_json_strings(value, text);
	}
}

fn collect_json_strings(value: &Value, text: &mut String) {
	match value {
		Value::String(value) => {
			text.push(' ');
			text.push_str(value);
		},
		Value::Array(values) =>
			for value in values {
				collect_json_strings(value, text);
			},
		Value::Object(object) => collect_json_strings_from_map(object, text),
		Value::Bool(_) | Value::Null | Value::Number(_) => {},
	}
}
