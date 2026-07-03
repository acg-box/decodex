//! Signal and config-feature artifact validation.

use serde_json::{Map, Value};

use crate::artifact_validation::{
	SIGNAL_CONFIDENCE,
	constants::{SIGNAL_IMPACT, SIGNAL_KINDS, SOURCE_ITEM_KINDS},
	support,
};

pub(super) fn validate_signal(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if support::string_field(entry, "lane") != Some("github") {
		errors.push("lane must be github for the MVP".into());
	}
	if !support::matches_one_of(entry.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", support::choices(SIGNAL_KINDS)));
	}
	if !support::matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", support::choices(SIGNAL_CONFIDENCE)));
	}
	if !support::matches_one_of(entry.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", support::choices(SIGNAL_IMPACT)));
	}

	for field in ["slug", "title", "published_at", "summary", "why_it_matters"] {
		if !support::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_signal_lists(entry, errors);
	validate_signal_try_fields(entry, errors);
	validate_signal_source_refs(entry.get("source_refs"), errors);
	validate_multi_agent_v2_reference_text(entry, "signal entries", errors);
}

pub(super) fn validate_config_feature_catalog(
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

pub(super) fn validate_multi_agent_v2_catalog_feature(
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
	if lower.contains("assign_task") && !has_legacy_multi_agent_v2_context(&lower) {
		errors.push(format!(
			"features[{index}].reference_description must label assign_task as legacy or renamed context"
		));
	}
}

pub(super) fn validate_multi_agent_v2_reference_text(
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

pub(super) fn collect_json_strings_from_map(object: &Map<String, Value>, text: &mut String) {
	for value in object.values() {
		collect_json_strings(value, text);
	}
}

pub(super) fn collect_json_strings(value: &Value, text: &mut String) {
	match value {
		Value::String(value) => {
			text.push(' ');
			text.push_str(value);
		},
		Value::Array(values) => {
			for value in values {
				collect_json_strings(value, text);
			}
		},
		Value::Object(object) => collect_json_strings_from_map(object, text),
		Value::Bool(_) | Value::Null | Value::Number(_) => {},
	}
}

pub(super) fn validate_signal_lists(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if support::non_empty_array(entry.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
}

pub(super) fn validate_signal_try_fields(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let config_flags_present =
		support::optional_array(entry.get("config_flags"), "config_flags", errors)
			.is_some_and(|values| !values.is_empty());
	let how_to_try = entry.get("how_to_try");

	if (support::string_field(entry, "kind") == Some("try_now") || config_flags_present)
		&& !support::is_truthy_json_value(how_to_try)
	{
		errors.push("how_to_try is required for try_now or flag-backed entries".into());
	}
	if support::is_truthy_json_value(how_to_try)
		&& !support::is_truthy_json_value(entry.get("expected_effect"))
	{
		errors.push("expected_effect is required when how_to_try is present".into());
	}

	support::validate_optional_string_list(entry.get("caveats"), "caveats", errors);

	let watch_state = entry.get("watch_state");

	if watch_state.is_some()
		&& !watch_state.is_some_and(|value| support::is_non_empty_string(Some(value)))
	{
		errors.push("watch_state must be a non-empty string when present".into());
	}
}

pub(super) fn validate_signal_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};

	if support::string_field(refs, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("source_refs.repo must be owner/name".into());
	}

	validate_signal_source_items(refs.get("items"), errors);

	let pr_url = refs.get("pr_url");
	let commit_urls = refs.get("commit_urls");
	let items = refs.get("items");

	if pr_url.is_none()
		&& support::is_empty_or_missing_array(commit_urls)
		&& support::is_empty_or_missing_array(items)
	{
		errors.push("source_refs must include pr_url, commit URLs, or source_refs.items".into());
	}
	if pr_url.is_some_and(|url| !support::is_https_string(Some(url))) {
		errors.push("source_refs.pr_url must be an https URL when present".into());
	}
	if commit_urls.is_some_and(|urls| !support::is_https_string_array(urls)) {
		errors.push("source_refs.commit_urls must be a list of https URLs".into());
	}
}

pub(super) fn validate_signal_source_items(items: Option<&Value>, errors: &mut Vec<String>) {
	let Some(items) = items else {
		return;
	};

	if items.as_array().is_some_and(Vec::is_empty) {
		return;
	}

	let valid = items.as_array().is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				support::matches_one_of(item.get("kind"), SOURCE_ITEM_KINDS)
					&& support::is_non_empty_string(item.get("title"))
					&& support::is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| meta.as_str().is_some())
			})
		})
	});

	if !valid {
		errors.push("source_refs.items must be a list of titled source entries".into());
	}
}

pub(crate) fn has_legacy_multi_agent_v2_context(text: &str) -> bool {
	["legacy", "historical", "older", "previous", "renamed", "rename"]
		.into_iter()
		.any(|term| text.contains(term))
}
