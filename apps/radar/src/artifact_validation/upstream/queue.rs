use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::{
	UPSTREAM_SUBJECT_KINDS,
	artifact_validation::{
		constants::{
			UPSTREAM_REVIEW_NEXT_STEPS, UPSTREAM_REVIEW_PRIORITIES, UPSTREAM_SOURCE_STATES,
		},
		support,
	},
};

pub(in crate::artifact_validation) fn validate_upstream_review_queue(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	if support::string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !support::is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}
	support::validate_rfc3339_field(entry, "generated_at", errors);

	validate_upstream_review_queue_source(entry.get("source"), errors);

	let subjects = validate_upstream_review_subjects(entry.get("subjects"), errors);

	validate_upstream_review_counts(entry.get("counts"), subjects, errors);
}

fn validate_upstream_review_queue_source(source: Option<&Value>, errors: &mut Vec<String>) {
	let Some(source) = source.and_then(Value::as_object) else {
		errors.push("source must be an object".into());

		return;
	};

	if !support::is_non_empty_string(source.get("default_branch")) {
		errors.push("source.default_branch must be a non-empty string".into());
	}
	support::validate_git_object_id(source.get("upstream_head"), "source.upstream_head", errors);
	if source.get("search_limit").and_then(Value::as_i64).is_none_or(|value| value < 1) {
		errors.push("source.search_limit must be a positive integer".into());
	}
}

fn validate_upstream_review_subjects(subjects: Option<&Value>, errors: &mut Vec<String>) -> usize {
	let Some(subjects) = subjects.and_then(Value::as_array) else {
		errors.push("subjects must be a list".into());

		return 0;
	};
	let mut seen = BTreeSet::new();

	for (index, subject) in subjects.iter().enumerate() {
		let Some(subject) = subject.as_object() else {
			errors.push(format!("subjects[{index}] must be an object"));

			continue;
		};

		validate_upstream_review_subject(subject, index, &mut seen, errors);
	}

	subjects.len()
}

fn validate_upstream_review_subject(
	subject: &Map<String, Value>,
	index: usize,
	seen: &mut BTreeSet<(String, String)>,
	errors: &mut Vec<String>,
) {
	let subject_kind = support::string_field(subject, "subject_kind");
	let subject_id = support::string_field(subject, "subject_id");

	if !support::matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subjects[{index}].subject_kind must be one of {}",
			support::choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !support::is_non_empty_string(subject.get("subject_id")) {
		errors.push(format!("subjects[{index}].subject_id must be a non-empty string"));
	}

	if let (Some(subject_kind), Some(subject_id)) = (subject_kind, subject_id) {
		let key = (subject_kind.to_owned(), subject_id.to_owned());

		if !seen.insert(key) {
			errors.push(format!("subjects[{index}] duplicates {subject_kind}:{subject_id}"));
		}
	}

	validate_upstream_review_subject_fields(subject, index, errors);
}

fn validate_upstream_review_subject_fields(
	subject: &Map<String, Value>,
	index: usize,
	errors: &mut Vec<String>,
) {
	for field in ["title", "url", "review_reason"] {
		if !support::is_non_empty_string(subject.get(field)) {
			errors.push(format!("subjects[{index}].{field} must be a non-empty string"));
		}
	}

	if !support::is_https_string(subject.get("url")) {
		errors.push(format!("subjects[{index}].url must be an https URL"));
	}
	if !support::matches_one_of(subject.get("source_state"), UPSTREAM_SOURCE_STATES) {
		errors.push(format!(
			"subjects[{index}].source_state must be one of {}",
			support::choices(UPSTREAM_SOURCE_STATES)
		));
	}
	if !support::matches_one_of(subject.get("review_priority"), UPSTREAM_REVIEW_PRIORITIES) {
		errors.push(format!(
			"subjects[{index}].review_priority must be one of {}",
			support::choices(UPSTREAM_REVIEW_PRIORITIES)
		));
	}
	if !support::matches_one_of(subject.get("next_step"), UPSTREAM_REVIEW_NEXT_STEPS) {
		errors.push(format!(
			"subjects[{index}].next_step must be one of {}",
			support::choices(UPSTREAM_REVIEW_NEXT_STEPS)
		));
	}

	support::validate_git_object_id_list(
		subject.get("commit_shas"),
		&format!("subjects[{index}].commit_shas"),
		errors,
	);

	for field in ["surface_hints", "attention_flags", "sample_paths"] {
		support::validate_optional_string_list(
			subject.get(field),
			&format!("subjects[{index}].{field}"),
			errors,
		);
	}

	if subject.get("changed_file_count").and_then(Value::as_i64).is_none_or(|value| value < 0) {
		errors.push(format!("subjects[{index}].changed_file_count must be a non-negative integer"));
	}
}

fn validate_upstream_review_counts(
	counts: Option<&Value>,
	subjects: usize,
	errors: &mut Vec<String>,
) {
	let Some(counts) = counts.and_then(Value::as_object) else {
		errors.push("counts must be an object".into());

		return;
	};

	if counts.get("subjects_queued").and_then(Value::as_u64) != Some(subjects as u64) {
		errors.push("counts.subjects_queued must equal len(subjects)".into());
	}

	for field in
		["recent_commits_scanned", "published_subjects_seen", "critical", "high", "normal", "low"]
	{
		if counts.get(field).and_then(Value::as_i64).is_none_or(|value| value < 0) {
			errors.push(format!("counts.{field} must be a non-negative integer"));
		}
	}
}
