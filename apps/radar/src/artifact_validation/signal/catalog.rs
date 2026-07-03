use serde_json::{Map, Value};

use crate::artifact_validation::{signal::text, support};

pub(crate) fn validate_config_feature_catalog(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	if !support::is_https_string(entry.get("source_url")) {
		errors.push("source_url must be an https URL".into());
	}
	if !support::is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	let Some(features) = support::non_empty_array(entry.get("features")) else {
		errors.push("features must be a non-empty list".into());

		return;
	};

	if entry
		.get("feature_count")
		.and_then(Value::as_u64)
		.is_none_or(|count| count != features.len() as u64)
	{
		errors.push("feature_count must match features length".into());
	}

	let mut found_multi_agent_v2 = false;

	for (index, feature) in features.iter().enumerate() {
		let Some(feature) = feature.as_object() else {
			errors.push(format!("features[{index}] must be an object"));

			continue;
		};

		for field in [
			"name",
			"config_path",
			"toml_assignment",
			"toml_snippet",
			"cli_enable_flag",
			"schema_url",
			"reference_url",
			"github_search_url",
		] {
			if !support::is_non_empty_string(feature.get(field)) {
				errors.push(format!("features[{index}].{field} must be a non-empty string"));
			}
		}

		if support::string_field(feature, "name") == Some("multi_agent_v2") {
			found_multi_agent_v2 = true;

			validate_multi_agent_v2_catalog_feature(feature, index, errors);
		}
	}

	if !found_multi_agent_v2 {
		errors.push("features must include multi_agent_v2".into());
	}
}

fn validate_multi_agent_v2_catalog_feature(
	feature: &Map<String, Value>,
	index: usize,
	errors: &mut Vec<String>,
) {
	let Some(description) = feature.get("reference_description").and_then(Value::as_str) else {
		errors.push(format!(
			"features[{index}].reference_description must describe current followup_task behavior"
		));

		return;
	};
	let lower = description.to_ascii_lowercase();

	if !lower.contains("followup_task") {
		errors.push(format!(
			"features[{index}].reference_description must mention current followup_task behavior"
		));
	}
	if lower.contains("assign_task") && !text::has_legacy_multi_agent_v2_context(&lower) {
		errors.push(format!(
			"features[{index}].reference_description must label assign_task as legacy or renamed context"
		));
	}
}
