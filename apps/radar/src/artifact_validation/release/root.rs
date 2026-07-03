use serde_json::{Map, Value};

use crate::artifact_validation::{
	release::{compare, options},
	support,
};

pub(crate) fn validate_release_delta(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if support::string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !support::is_non_empty_string(entry.get("tag_prefix")) {
		errors.push("tag_prefix must be a non-empty string".into());
	}
	if !support::is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	let tag_prefix = support::string_field(entry, "tag_prefix").unwrap_or_default();

	compare::validate_release_object(
		entry.get("stable_release"),
		"stable_release",
		tag_prefix,
		false,
		errors,
	);
	compare::validate_release_object(
		entry.get("prerelease"),
		"prerelease",
		tag_prefix,
		true,
		errors,
	);
	compare::validate_compare_object(entry.get("compare"), "compare", errors);
	support::validate_string_list(
		entry.get("tracked_signal_slugs"),
		"tracked_signal_slugs",
		errors,
	);

	let option_tags = options::validate_release_options(entry.get("release_options"), errors);

	compare::validate_release_comparisons(entry, &option_tags, errors);
}
