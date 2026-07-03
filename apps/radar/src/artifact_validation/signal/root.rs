use serde_json::{Map, Value};

use crate::artifact_validation::{
	SIGNAL_CONFIDENCE,
	constants::{SIGNAL_IMPACT, SIGNAL_KINDS},
	signal::{references, text},
	support,
};

pub(crate) fn validate_signal(entry: &Map<String, Value>, errors: &mut Vec<String>) {
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

	references::validate_signal_source_refs(entry.get("source_refs"), errors);
	text::validate_multi_agent_v2_reference_text(entry, "signal entries", errors);
}

fn validate_signal_lists(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if support::non_empty_array(entry.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
}

fn validate_signal_try_fields(entry: &Map<String, Value>, errors: &mut Vec<String>) {
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
