//! GitHub bundle schema validation.

use serde_json::{Map, Value};

use crate::artifact_validation::{constants::ANALYSIS_MODES, support};

pub(super) fn validate_bundle(bundle: &Map<String, Value>, errors: &mut Vec<String>) {
	if support::string_field(bundle, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !support::matches_one_of(bundle.get("analysis_mode"), ANALYSIS_MODES) {
		errors.push(format!("analysis_mode must be one of {}", support::choices(ANALYSIS_MODES)));
	}
	if !support::is_non_empty_string(bundle.get("default_branch")) {
		errors.push("default_branch must be a non-empty string".into());
	}

	validate_bundle_commits(bundle.get("commits"), errors);
	validate_bundle_files(bundle.get("files"), errors);

	if support::string_field(bundle, "analysis_mode") == Some("pr_first") {
		validate_bundle_pr(bundle.get("primary_pr"), errors);
	}
}

pub(super) fn validate_bundle_commits(commits: Option<&Value>, errors: &mut Vec<String>) {
	let Some(commits) = support::non_empty_array(commits) else {
		errors.push("commits must be a non-empty list".into());

		return;
	};

	for (index, commit) in commits.iter().enumerate() {
		let Some(commit) = commit.as_object() else {
			errors.push(format!("commits[{index}] must be an object"));

			continue;
		};

		for field in ["sha", "message", "url"] {
			if !support::is_non_empty_string(commit.get(field)) {
				errors.push(format!("commits[{index}].{field} must be a non-empty string"));
			}
		}
	}
}

pub(super) fn validate_bundle_files(files: Option<&Value>, errors: &mut Vec<String>) {
	let Some(files) = support::non_empty_array(files) else {
		errors.push("files must be a non-empty list".into());

		return;
	};

	for (index, item) in files.iter().enumerate() {
		let Some(item) = item.as_object() else {
			errors.push(format!("files[{index}] must be an object"));

			continue;
		};

		for field in ["path", "status", "additions", "deletions"] {
			let Some(value) = item.get(field) else {
				errors.push(format!("files[{index}].{field} is required"));
				continue;
			};
			let (valid, expected) = if matches!(field, "path" | "status") {
				(support::is_non_empty_string(Some(value)), "a non-empty string")
			} else {
				(value.as_i64().is_some_and(|value| value >= 0), "a non-negative integer")
			};
			if !valid {
				errors.push(format!("files[{index}].{field} must be {expected}"));
			}
		}
	}
}

pub(super) fn validate_bundle_pr(primary_pr: Option<&Value>, errors: &mut Vec<String>) {
	let Some(primary_pr) = primary_pr.and_then(Value::as_object) else {
		errors.push("primary_pr is required when analysis_mode is pr_first".into());

		return;
	};

	for field in ["number", "title", "body", "state", "labels", "url"] {
		let Some(value) = primary_pr.get(field) else {
			errors.push(format!("primary_pr.{field} is required"));
			continue;
		};
		let (valid, expected) = match field {
			"number" => (value.as_i64().is_some_and(|value| value > 0), "a positive integer"),
			"body" => (value.as_str().is_some(), "a string"),
			"labels" => (
				value.as_array().is_some_and(|values| {
					values.iter().all(|value| support::is_non_empty_string(Some(value)))
				}),
				"a list of non-empty strings",
			),
			_ => (support::is_non_empty_string(Some(value)), "a non-empty string"),
		};
		if !valid {
			errors.push(format!("primary_pr.{field} must be {expected}"));
		}
	}
}
