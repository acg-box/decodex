//! Upstream review, impact, and control-plane validation.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
	SIGNAL_CONFIDENCE, UPSTREAM_SUBJECT_KINDS,
	constants::{
		CODEX_COMPATIBILITY_STATUSES, CODEX_TARGET_CHANNELS, CONTROL_PLANE_UPGRADE_IMPACTS,
		CONTROL_PLANE_UPGRADE_PATHS, CONTROL_PLANE_UPGRADE_STATUSES, UPSTREAM_IMPACT_KINDS,
		UPSTREAM_REVIEW_ACTION_TYPES, UPSTREAM_REVIEW_NEXT_STEPS, UPSTREAM_REVIEW_PRIORITIES,
		UPSTREAM_SOURCE_STATES,
	},
	model::ArtifactValidationOptions,
	support::{
		choices, is_https_string, is_https_string_array, is_non_empty_string, matches_one_of,
		non_empty_array, string_field, validate_non_empty_string_list,
		validate_optional_string_list,
	},
};

pub(super) fn validate_upstream_review_queue(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	validate_upstream_review_queue_source(entry.get("source"), errors);

	let subjects = validate_upstream_review_subjects(entry.get("subjects"), errors);

	validate_upstream_review_counts(entry.get("counts"), subjects, errors);
}

pub(super) fn validate_upstream_review_queue_source(
	source: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(source) = source.and_then(Value::as_object) else {
		errors.push("source must be an object".into());

		return;
	};

	if !is_non_empty_string(source.get("default_branch")) {
		errors.push("source.default_branch must be a non-empty string".into());
	}
	if source.get("search_limit").and_then(Value::as_i64).is_none_or(|value| value < 1) {
		errors.push("source.search_limit must be a positive integer".into());
	}
}

pub(super) fn validate_upstream_review_subjects(
	subjects: Option<&Value>,
	errors: &mut Vec<String>,
) -> usize {
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

pub(super) fn validate_upstream_review_subject(
	subject: &Map<String, Value>,
	index: usize,
	seen: &mut BTreeSet<(String, String)>,
	errors: &mut Vec<String>,
) {
	let subject_kind = string_field(subject, "subject_kind");
	let subject_id = string_field(subject, "subject_id");

	if !matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subjects[{index}].subject_kind must be one of {}",
			choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !is_non_empty_string(subject.get("subject_id")) {
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

pub(super) fn validate_upstream_review_subject_fields(
	subject: &Map<String, Value>,
	index: usize,
	errors: &mut Vec<String>,
) {
	for field in ["title", "url", "review_reason"] {
		if !is_non_empty_string(subject.get(field)) {
			errors.push(format!("subjects[{index}].{field} must be a non-empty string"));
		}
	}

	if !is_https_string(subject.get("url")) {
		errors.push(format!("subjects[{index}].url must be an https URL"));
	}
	if !matches_one_of(subject.get("source_state"), UPSTREAM_SOURCE_STATES) {
		errors.push(format!(
			"subjects[{index}].source_state must be one of {}",
			choices(UPSTREAM_SOURCE_STATES)
		));
	}
	if !matches_one_of(subject.get("review_priority"), UPSTREAM_REVIEW_PRIORITIES) {
		errors.push(format!(
			"subjects[{index}].review_priority must be one of {}",
			choices(UPSTREAM_REVIEW_PRIORITIES)
		));
	}
	if !matches_one_of(subject.get("next_step"), UPSTREAM_REVIEW_NEXT_STEPS) {
		errors.push(format!(
			"subjects[{index}].next_step must be one of {}",
			choices(UPSTREAM_REVIEW_NEXT_STEPS)
		));
	}

	validate_non_empty_string_list(
		subject.get("commit_shas"),
		&format!("subjects[{index}].commit_shas"),
		errors,
	);

	for field in ["surface_hints", "attention_flags", "sample_paths"] {
		validate_optional_string_list(
			subject.get(field),
			&format!("subjects[{index}].{field}"),
			errors,
		);
	}

	if subject.get("changed_file_count").and_then(Value::as_i64).is_none_or(|value| value < 0) {
		errors.push(format!("subjects[{index}].changed_file_count must be a non-negative integer"));
	}
}

pub(super) fn validate_upstream_review_counts(
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

pub(super) fn validate_upstream_review(
	entry: &Map<String, Value>,
	options: ArtifactValidationOptions,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "repo", "reviewed_at", "observed_change"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_review_subject_object(entry.get("subject"), errors);
	validate_upstream_review_source_refs(entry.get("source_refs"), errors);

	for field in ["changed_surfaces", "evidence"] {
		validate_non_empty_string_list(entry.get(field), field, errors);
	}

	validate_upstream_review_optional_strings(entry, errors);

	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}

	validate_upstream_review_actions(entry.get("next_actions"), options, errors);
}

pub(super) fn validate_upstream_review_subject_object(
	subject: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(subject) = subject.and_then(Value::as_object) else {
		errors.push("subject must be an object".into());

		return;
	};

	if !matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subject.subject_kind must be one of {}",
			choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !is_non_empty_string(subject.get("subject_id")) {
		errors.push("subject.subject_id must be a non-empty string".into());
	}

	validate_optional_string_list(subject.get("commit_shas"), "subject.commit_shas", errors);
}

pub(super) fn validate_upstream_review_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				is_non_empty_string(item.get("kind"))
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}

pub(super) fn validate_upstream_review_optional_strings(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
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

pub(super) fn validate_upstream_review_actions(
	next_actions: Option<&Value>,
	options: ArtifactValidationOptions,
	errors: &mut Vec<String>,
) {
	let Some(next_actions) = non_empty_array(next_actions) else {
		errors.push("next_actions must be a non-empty list".into());

		return;
	};

	for (index, action) in next_actions.iter().enumerate() {
		let Some(action) = action.as_object() else {
			errors.push(format!("next_actions[{index}] must be an object"));

			continue;
		};

		let legacy_linear_followup = options.allow_historical_upstream_review_linear_followup
			&& string_field(action, "type") == Some("linear_followup");

		if !legacy_linear_followup
			&& !matches_one_of(action.get("type"), UPSTREAM_REVIEW_ACTION_TYPES)
		{
			errors.push(format!(
				"next_actions[{index}].type must be one of {}",
				choices(UPSTREAM_REVIEW_ACTION_TYPES)
			));
		}
		if !is_non_empty_string(action.get("reason")) {
			errors.push(format!("next_actions[{index}].reason must be a non-empty string"));
		}
	}
}

pub(super) fn validate_upstream_impact(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "observed_change"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_impact_source_refs(entry.get("source_refs"), errors);

	if !matches_one_of(entry.get("public_signal_decision"), &["defer", "publish", "skip"]) {
		errors.push("public_signal_decision must be one of ['defer', 'publish', 'skip']".into());
	}
	if !matches_one_of(
		entry.get("control_plane_impact"),
		&["adopt_now", "candidate", "compat_risk", "none", "watch"],
	) {
		errors.push("control_plane_impact must be one of ['adopt_now', 'candidate', 'compat_risk', 'none', 'watch']".into());
	}
	if !matches_one_of(
		entry.get("publisher_angle"),
		&["none", "operator_impact", "practical_explainer", "release_pulse", "watch_note"],
	) {
		errors.push("publisher_angle must be one of ['none', 'operator_impact', 'practical_explainer', 'release_pulse', 'watch_note']".into());
	}
	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}

	validate_non_empty_string_list(entry.get("evidence"), "evidence", errors);

	for field in ["candidate_followups", "social_notes", "caveats"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

pub(super) fn validate_control_plane_upgrade_candidate(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "repo", "observed_change", "reason"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !matches_one_of(entry.get("status"), CONTROL_PLANE_UPGRADE_STATUSES) {
		errors.push(format!("status must be one of {}", choices(CONTROL_PLANE_UPGRADE_STATUSES)));
	}
	if !matches_one_of(entry.get("control_plane_impact"), CONTROL_PLANE_UPGRADE_IMPACTS) {
		errors.push(format!(
			"control_plane_impact must be one of {}",
			choices(CONTROL_PLANE_UPGRADE_IMPACTS)
		));
	}
	if !matches_one_of(entry.get("upgrade_path"), CONTROL_PLANE_UPGRADE_PATHS) {
		errors
			.push(format!("upgrade_path must be one of {}", choices(CONTROL_PLANE_UPGRADE_PATHS)));
	}

	validate_control_plane_upgrade_source_refs(entry.get("source_refs"), errors);
	validate_control_plane_upgrade_target_codex(entry.get("target_codex"), errors);
	validate_control_plane_upgrade_authority(entry.get("authority"), errors);
	validate_non_empty_string_list(entry.get("affected_surfaces"), "affected_surfaces", errors);
	validate_non_empty_string_list(entry.get("validation_gates"), "validation_gates", errors);
	validate_non_empty_string_list(entry.get("stop_conditions"), "stop_conditions", errors);

	for field in ["acceptance_criteria", "caveats", "next_steps"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

pub(super) fn validate_control_plane_upgrade_source_refs(
	refs: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = ["upstream_reviews", "upstream_impacts", "release_deltas", "urls"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, release_deltas, or urls"
				.into(),
		);
	}
	if non_empty_array(refs.get("upstream_impacts")).is_none() {
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "release_deltas"] {
		validate_optional_string_list(refs.get(field), &format!("source_refs.{field}"), errors);
	}
}

pub(super) fn validate_control_plane_upgrade_target_codex(
	target: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(target) = target.and_then(Value::as_object) else {
		errors.push("target_codex must be an object".into());

		return;
	};

	if !matches_one_of(target.get("channel"), CODEX_TARGET_CHANNELS) {
		errors.push(format!(
			"target_codex.channel must be one of {}",
			choices(CODEX_TARGET_CHANNELS)
		));
	}
	if !["version", "tag", "commit_sha", "release_url"]
		.iter()
		.any(|field| is_non_empty_string(target.get(*field)))
	{
		errors.push("target_codex must include version, tag, commit_sha, or release_url".into());
	}
	if target.get("release_url").is_some_and(|url| !is_https_string(Some(url))) {
		errors.push("target_codex.release_url must be an https URL when present".into());
	}
	if target
		.get("compatibility_status")
		.is_some_and(|status| !matches_one_of(Some(status), CODEX_COMPATIBILITY_STATUSES))
	{
		errors.push(format!(
			"target_codex.compatibility_status must be one of {}",
			choices(CODEX_COMPATIBILITY_STATUSES)
		));
	}

	for field in ["version", "tag", "commit_sha", "matrix_ref", "probe_evidence"] {
		if target.get(field).is_some_and(|value| !is_non_empty_string(Some(value))) {
			errors.push(format!("target_codex.{field} must be non-empty when present"));
		}
	}
}

pub(super) fn validate_control_plane_upgrade_authority(
	authority: Option<&Value>,
	errors: &mut Vec<String>,
) {
	let Some(authority) = authority.and_then(Value::as_object) else {
		errors.push("authority must be an object".into());

		return;
	};

	for field in ["decision_contract_required", "program_intake_required"] {
		if authority.get(field).and_then(Value::as_bool) != Some(true) {
			errors.push(format!("authority.{field} must be true"));
		}
	}

	if authority.get("mutation_allowed").and_then(Value::as_bool) != Some(false) {
		errors.push("authority.mutation_allowed must be false".into());
	}

	for field in ["objective_id", "objective_version", "policy_ref"] {
		if authority.get(field).is_some_and(|value| !is_non_empty_string(Some(value))) {
			errors.push(format!("authority.{field} must be non-empty when present"));
		}
	}
}

pub(super) fn validate_upstream_impact_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				matches_one_of(item.get("kind"), UPSTREAM_IMPACT_KINDS)
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| is_non_empty_string(Some(meta)))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}
