//! Release delta artifact validation.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::artifact_validation::{
	model::ReleaseOptionTags,
	support::{self},
};

pub(super) fn validate_release_delta(entry: &Map<String, Value>, errors: &mut Vec<String>) {
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

	validate_release_object(
		entry.get("stable_release"),
		"stable_release",
		tag_prefix,
		false,
		errors,
	);
	validate_release_object(entry.get("prerelease"), "prerelease", tag_prefix, true, errors);
	validate_compare_object(entry.get("compare"), "compare", errors);

	support::validate_string_list(
		entry.get("tracked_signal_slugs"),
		"tracked_signal_slugs",
		errors,
	);

	let option_tags = validate_release_options(entry.get("release_options"), errors);

	validate_release_comparisons(entry, &option_tags, errors);
}

pub(super) fn validate_release_object(
	release: Option<&Value>,
	field_name: &str,
	tag_prefix: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
) {
	let Some(release) = release.and_then(Value::as_object) else {
		errors.push(format!("{field_name} must be an object"));

		return;
	};

	for field in ["tag_name", "name", "published_at", "url"] {
		if !support::is_non_empty_string(release.get(field)) {
			errors.push(format!("{field_name}.{field} must be a non-empty string"));
		}
	}

	if support::string_field(release, "tag_name")
		.is_some_and(|tag_name| !tag_name.starts_with(tag_prefix))
	{
		errors.push(format!("{field_name}.tag_name must start with tag_prefix"));
	}
	if release.get("prerelease").and_then(Value::as_bool) != Some(expect_prerelease) {
		let expected = if expect_prerelease { "true" } else { "false" };

		errors.push(format!("{field_name}.prerelease must be {expected}"));
	}
}

pub(super) fn validate_compare_object(
	compare: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let Some(compare) = compare.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object"));

		return;
	};

	if !support::is_non_empty_string(compare.get("status")) {
		errors.push(format!("{label}.status must be a non-empty string"));
	}

	for field in ["ahead_by", "total_commits"] {
		if compare.get(field).and_then(Value::as_i64).is_none() {
			errors.push(format!("{label}.{field} must be an integer"));
		}
	}

	if !support::is_https_string(compare.get("url")) {
		errors.push(format!("{label}.url must be an https URL"));
	}

	support::validate_optional_string_list(
		compare.get("commit_shas"),
		&format!("{label}.commit_shas"),
		errors,
	);
	support::validate_optional_positive_integer_list(
		compare.get("pr_numbers"),
		&format!("{label}.pr_numbers"),
		errors,
	);
}

pub(super) fn validate_release_options(
	options: Option<&Value>,
	errors: &mut Vec<String>,
) -> ReleaseOptionTags {
	let mut tags = ReleaseOptionTags::default();
	let Some(options) = options.and_then(Value::as_object) else {
		errors.push("release_options must be an object".into());

		return tags;
	};

	validate_release_option_group(
		options.get("stable"),
		"release_options.stable",
		false,
		errors,
		&mut tags.stable,
	);
	validate_release_option_group(
		options.get("preview"),
		"release_options.preview",
		true,
		errors,
		&mut tags.preview,
	);

	tags
}

pub(super) fn validate_release_option_group(
	values: Option<&Value>,
	label: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
	tags: &mut BTreeSet<String>,
) {
	let Some(values) = support::non_empty_array(values) else {
		errors.push(format!("{label} must be a non-empty list"));

		return;
	};

	for (index, release) in values.iter().enumerate() {
		let Some(release) = release.as_object() else {
			errors.push(format!("{label}[{index}] must be an object"));

			continue;
		};

		if let Some(tag_name) = support::string_field(release, "tag_name") {
			if tag_name.is_empty() {
				errors.push(format!("{label}[{index}].tag_name must be a non-empty string"));
			} else {
				tags.insert(tag_name.to_owned());
			}
		} else {
			errors.push(format!("{label}[{index}].tag_name must be a non-empty string"));
		}

		if release.get("prerelease").and_then(Value::as_bool) != Some(expect_prerelease) {
			let expected = if expect_prerelease { "true" } else { "false" };

			errors.push(format!("{label}[{index}].prerelease must be {expected}"));
		}
	}
}

pub(super) fn validate_release_comparisons(
	entry: &Map<String, Value>,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	let Some(comparisons) = support::non_empty_array(entry.get("comparisons")) else {
		errors.push("comparisons must be a non-empty list".into());

		return;
	};
	let stable_release = entry.get("stable_release").and_then(Value::as_object);
	let prerelease = entry.get("prerelease").and_then(Value::as_object);
	let mut has_default_comparison = false;

	for (index, comparison) in comparisons.iter().enumerate() {
		let Some(comparison) = comparison.as_object() else {
			errors.push(format!("comparisons[{index}] must be an object"));

			continue;
		};

		validate_release_comparison_tags(comparison, index, option_tags, errors);

		if comparison_matches_default(comparison, stable_release, prerelease) {
			has_default_comparison = true;
		}

		validate_compare_object(
			comparison.get("compare"),
			&format!("comparisons[{index}].compare"),
			errors,
		);

		support::validate_string_list(
			comparison.get("tracked_signal_slugs"),
			&format!("comparisons[{index}].tracked_signal_slugs"),
			errors,
		);
	}

	if !has_default_comparison {
		errors.push("comparisons must include the default stable/prerelease pair".into());
	}
}

pub(super) fn validate_release_comparison_tags(
	comparison: &Map<String, Value>,
	index: usize,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	match support::string_field(comparison, "stable_tag_name") {
		Some("") => {
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string"))
		},
		Some(tag_name)
			if !option_tags.stable.is_empty() && !option_tags.stable.contains(tag_name) =>
		{
			errors.push(format!(
				"comparisons[{index}].stable_tag_name must exist in release_options.stable"
			))
		},
		Some(_) => {},
		None => {
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string"))
		},
	}
	match support::string_field(comparison, "prerelease_tag_name") {
		Some("") => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
		Some(tag_name)
			if !option_tags.preview.is_empty() && !option_tags.preview.contains(tag_name) =>
		{
			errors.push(format!(
				"comparisons[{index}].prerelease_tag_name must exist in release_options.preview"
			))
		},
		Some(_) => {},
		None => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
	}
}

pub(super) fn comparison_matches_default(
	comparison: &Map<String, Value>,
	stable_release: Option<&Map<String, Value>>,
	prerelease: Option<&Map<String, Value>>,
) -> bool {
	let stable_tag = stable_release.and_then(|release| support::string_field(release, "tag_name"));
	let prerelease_tag = prerelease.and_then(|release| support::string_field(release, "tag_name"));

	support::string_field(comparison, "stable_tag_name") == stable_tag
		&& support::string_field(comparison, "prerelease_tag_name") == prerelease_tag
}
