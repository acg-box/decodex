use serde_json::{Map, Value};

use crate::{
	SIGNAL_CONFIDENCE, UPSTREAM_SUBJECT_KINDS,
	artifact_validation::{constants::UPSTREAM_REVIEW_ACTION_TYPES, support},
};

pub(in crate::artifact_validation) fn validate_upstream_review(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "repo", "reviewed_at", "observed_change"] {
		if !support::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}
	support::validate_rfc3339_field(entry, "reviewed_at", errors);
	support::validate_git_object_id(entry.get("upstream_head"), "upstream_head", errors);

	if support::string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_review_subject_object(entry.get("subject"), errors);
	validate_upstream_review_source_refs(entry.get("source_refs"), errors);

	for field in ["changed_surfaces", "evidence"] {
		support::validate_non_empty_string_list(entry.get(field), field, errors);
	}

	validate_upstream_review_optional_strings(entry, errors);

	if !support::matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", support::choices(SIGNAL_CONFIDENCE)));
	}

	validate_upstream_review_actions(entry.get("next_actions"), errors);
}

fn validate_upstream_review_subject_object(subject: Option<&Value>, errors: &mut Vec<String>) {
	let Some(subject) = subject.and_then(Value::as_object) else {
		errors.push("subject must be an object".into());

		return;
	};

	if !support::matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subject.subject_kind must be one of {}",
			support::choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !support::is_non_empty_string(subject.get("subject_id")) {
		errors.push("subject.subject_id must be a non-empty string".into());
	}

	support::validate_git_object_id_list(subject.get("commit_shas"), "subject.commit_shas", errors);
}

fn validate_upstream_review_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = support::non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				support::is_non_empty_string(item.get("kind"))
					&& support::is_non_empty_string(item.get("title"))
					&& support::is_https_string(item.get("url"))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}

fn validate_upstream_review_optional_strings(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in [
		"user_visible_path",
		"control_plane_relevance",
		"compatibility_risk",
		"adoption_opportunity",
		"community_value",
		"deprecated_or_breaking_notes",
		"caveats",
	] {
		if entry.get(field).is_some_and(|value| !value.is_string() && !value.is_null()) {
			errors.push(format!("{field} must be a string when present"));
		}
	}
}

fn validate_upstream_review_actions(next_actions: Option<&Value>, errors: &mut Vec<String>) {
	let Some(next_actions) = support::non_empty_array(next_actions) else {
		errors.push("next_actions must be a non-empty list".into());

		return;
	};

	for (index, action) in next_actions.iter().enumerate() {
		let Some(action) = action.as_object() else {
			errors.push(format!("next_actions[{index}] must be an object"));

			continue;
		};
		if !support::matches_one_of(action.get("type"), UPSTREAM_REVIEW_ACTION_TYPES) {
			errors.push(format!(
				"next_actions[{index}].type must be one of {}",
				support::choices(UPSTREAM_REVIEW_ACTION_TYPES)
			));
		}
		if !support::is_non_empty_string(action.get("reason")) {
			errors.push(format!("next_actions[{index}].reason must be a non-empty string"));
		}
	}
}
