use serde_json::{Map, Value};

use crate::artifact_validation::{
	constants::{
		CODEX_COMPATIBILITY_STATUSES, CODEX_TARGET_CHANNELS, CONTROL_PLANE_UPGRADE_IMPACTS,
		CONTROL_PLANE_UPGRADE_PATHS, CONTROL_PLANE_UPGRADE_STATUSES,
	},
	support,
};

pub(in crate::artifact_validation) fn validate_control_plane_upgrade_candidate(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "repo", "observed_change", "reason"] {
		if !support::is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if support::string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !support::matches_one_of(entry.get("status"), CONTROL_PLANE_UPGRADE_STATUSES) {
		errors.push(format!(
			"status must be one of {}",
			support::choices(CONTROL_PLANE_UPGRADE_STATUSES)
		));
	}
	if !support::matches_one_of(entry.get("control_plane_impact"), CONTROL_PLANE_UPGRADE_IMPACTS) {
		errors.push(format!(
			"control_plane_impact must be one of {}",
			support::choices(CONTROL_PLANE_UPGRADE_IMPACTS)
		));
	}
	if !support::matches_one_of(entry.get("upgrade_path"), CONTROL_PLANE_UPGRADE_PATHS) {
		errors.push(format!(
			"upgrade_path must be one of {}",
			support::choices(CONTROL_PLANE_UPGRADE_PATHS)
		));
	}

	validate_control_plane_upgrade_source_refs(entry.get("source_refs"), errors);
	validate_control_plane_upgrade_target_codex(entry.get("target_codex"), errors);
	validate_control_plane_upgrade_authority(entry.get("authority"), errors);

	support::validate_non_empty_string_list(
		entry.get("affected_surfaces"),
		"affected_surfaces",
		errors,
	);
	support::validate_non_empty_string_list(
		entry.get("validation_gates"),
		"validation_gates",
		errors,
	);
	support::validate_non_empty_string_list(
		entry.get("stop_conditions"),
		"stop_conditions",
		errors,
	);

	for field in ["acceptance_criteria", "caveats", "next_steps"] {
		support::validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_control_plane_upgrade_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = ["upstream_reviews", "upstream_impacts", "release_deltas", "urls"]
		.iter()
		.any(|field| support::non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, release_deltas, or urls"
				.into(),
		);
	}
	if support::non_empty_array(refs.get("upstream_impacts")).is_none() {
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !support::is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "release_deltas"] {
		support::validate_optional_string_list(
			refs.get(field),
			&format!("source_refs.{field}"),
			errors,
		);
	}
}

fn validate_control_plane_upgrade_target_codex(target: Option<&Value>, errors: &mut Vec<String>) {
	let Some(target) = target.and_then(Value::as_object) else {
		errors.push("target_codex must be an object".into());

		return;
	};

	if !support::matches_one_of(target.get("channel"), CODEX_TARGET_CHANNELS) {
		errors.push(format!(
			"target_codex.channel must be one of {}",
			support::choices(CODEX_TARGET_CHANNELS)
		));
	}
	if !["version", "tag", "commit_sha", "release_url"]
		.iter()
		.any(|field| support::is_non_empty_string(target.get(*field)))
	{
		errors.push("target_codex must include version, tag, commit_sha, or release_url".into());
	}
	if target.get("release_url").is_some_and(|url| !support::is_https_string(Some(url))) {
		errors.push("target_codex.release_url must be an https URL when present".into());
	}
	if target
		.get("compatibility_status")
		.is_some_and(|status| !support::matches_one_of(Some(status), CODEX_COMPATIBILITY_STATUSES))
	{
		errors.push(format!(
			"target_codex.compatibility_status must be one of {}",
			support::choices(CODEX_COMPATIBILITY_STATUSES)
		));
	}

	for field in ["version", "tag", "commit_sha", "matrix_ref", "probe_evidence"] {
		if target.get(field).is_some_and(|value| !support::is_non_empty_string(Some(value))) {
			errors.push(format!("target_codex.{field} must be non-empty when present"));
		}
	}
}

fn validate_control_plane_upgrade_authority(authority: Option<&Value>, errors: &mut Vec<String>) {
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
		if authority.get(field).is_some_and(|value| !support::is_non_empty_string(Some(value))) {
			errors.push(format!("authority.{field} must be non-empty when present"));
		}
	}
}
