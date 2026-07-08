use serde_json::{Map, Value};

use crate::artifact_validation::{model::ReleaseOptionTags, release::options, support};

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

pub(super) fn validate_release_object(
	release: Option<&Value>,
	field_name: &str,
	tag_prefix: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
) {
	options::validate_release_object(release, field_name, tag_prefix, expect_prerelease, errors);
}

fn validate_release_comparison_tags(
	comparison: &Map<String, Value>,
	index: usize,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	match support::string_field(comparison, "stable_tag_name") {
		Some("") =>
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string")),
		Some(tag_name)
			if !option_tags.stable.is_empty() && !option_tags.stable.contains(tag_name) =>
			errors.push(format!(
				"comparisons[{index}].stable_tag_name must exist in release_options.stable"
			)),
		Some(_) => {},
		None =>
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string")),
	}
	match support::string_field(comparison, "prerelease_tag_name") {
		Some("") => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
		Some(tag_name)
			if !option_tags.preview.is_empty() && !option_tags.preview.contains(tag_name) =>
			errors.push(format!(
				"comparisons[{index}].prerelease_tag_name must exist in release_options.preview"
			)),
		Some(_) => {},
		None => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
	}
}

fn comparison_matches_default(
	comparison: &Map<String, Value>,
	stable_release: Option<&Map<String, Value>>,
	prerelease: Option<&Map<String, Value>>,
) -> bool {
	let stable_tag = stable_release.and_then(|release| support::string_field(release, "tag_name"));
	let prerelease_tag = prerelease.and_then(|release| support::string_field(release, "tag_name"));

	support::string_field(comparison, "stable_tag_name") == stable_tag
		&& support::string_field(comparison, "prerelease_tag_name") == prerelease_tag
}
