use std::collections::BTreeSet;

use serde_json::Value;

use crate::artifact_validation::{model::ReleaseOptionTags, support};

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

fn validate_release_option_group(
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
