//! Atomic quality-skip terminalization.

use std::{fs, path::Path};

use serde_json::{Value, json};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SocialTerminalizeSkipReport, SocialTerminalizeSkipRequest,
	prelude::{Result, eyre},
};

pub(crate) fn terminalize_social_skip(
	request: &SocialTerminalizeSkipRequest,
) -> Result<SocialTerminalizeSkipReport> {
	if request.daily_limit != 8 {
		return Err(eyre::eyre!("daily_limit must be 8"));
	}
	if request.timezone.trim().is_empty() {
		return Err(eyre::eyre!("timezone is required"));
	}
	if !valid_day(&request.day) {
		return Err(eyre::eyre!("day must use YYYY-MM-DD"));
	}

	let root = crate::repo_root()?;
	let candidates_dir = crate::resolve_against(&root, &request.candidates_dir);
	let candidate_path = crate::resolve_against(&root, &request.candidate_path);
	require_contained_regular_file(&candidate_path, &candidates_dir)?;
	let candidate = crate::load_json(&candidate_path)?;
	crate::validate_generated_social_artifact(&candidate)
		.map_err(|error| eyre::eyre!("candidate failed validation: {error}"))?;
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA) {
		return Err(eyre::eyre!("candidate must use {SOCIAL_CANDIDATE_SCHEMA}"));
	}

	let decision = candidate
		.get("decision")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	if decision.get("worthiness").and_then(Value::as_str) != Some("skip") {
		return Err(eyre::eyre!("candidate decision.worthiness must be skip"));
	}
	let idempotency_key = required_string(decision.get("idempotency_key"), "idempotency_key")?;
	let reason = required_string(decision.get("reason"), "reason")?;
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let reservations_dir = crate::resolve_against(&root, &request.reservations_dir);
	let output_path = posts_dir
		.join(&request.day)
		.join(format!("{}.json", crate::social_publish::idempotency_digest(idempotency_key)));
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)?;
	let scan = crate::social_publish::scan::scan_social_publish_state(
		&reservations_dir,
		&posts_dir,
		idempotency_key,
		&request.day,
	)?;
	let payload = skipped_post_payload(
		&candidate,
		crate::path_arg(&root, &candidate_path),
		reason,
		&request.day,
		&request.timezone,
		scan.published_count,
	)?;
	crate::validate_generated_social_artifact(&payload)
		.map_err(|error| eyre::eyre!("generated skipped post failed validation: {error}"))?;

	if output_path.exists() {
		if scan.idempotency_conflict.as_ref().is_some_and(|conflict| conflict != &output_path) {
			return Err(eyre::eyre!(
				"idempotency_key has another active reservation or terminal post: {}",
				idempotency_key
			));
		}
		return existing_result(
			&root,
			&candidate_path,
			&output_path,
			&payload,
			idempotency_key,
			scan.published_count,
		);
	}
	if let Some(conflict) = scan.idempotency_conflict {
		return Err(eyre::eyre!(
			"idempotency_key already has an active reservation or terminal post: {} ({})",
			idempotency_key,
			crate::path_arg(&root, &conflict)
		));
	}
	if !request.dry_run
		&& let Err(error) = crate::write_new_json(&output_path, &payload)
	{
		if output_path.exists() {
			return existing_result(
				&root,
				&candidate_path,
				&output_path,
				&payload,
				idempotency_key,
				scan.published_count,
			);
		}

		return Err(error);
	}

	Ok(SocialTerminalizeSkipReport {
		status: if request.dry_run { "dry_run".into() } else { "skipped".into() },
		path: crate::path_arg(&root, &output_path),
		candidate: crate::path_arg(&root, &candidate_path),
		idempotency_key: idempotency_key.into(),
		published_count: scan.published_count,
	})
}

fn skipped_post_payload(
	candidate: &Value,
	candidate_ref: String,
	reason: &str,
	day: &str,
	timezone: &str,
	published_count: usize,
) -> Result<Value> {
	let decision = candidate
		.get("decision")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;

	Ok(json!({
		"schema": "social_post/v1",
		"slug": required_string(candidate.get("slug"), "slug")?,
		"channel": "x",
		"target_account": "decodexspace",
		"controller_account": "hackink",
		"mode": required_string(candidate.get("mode"), "mode")?,
		"status": "skipped",
		"browser_touched": false,
		"audience": required_string(candidate.get("audience"), "audience")?,
		"text": candidate.get("candidate_text").cloned().ok_or_else(|| eyre::eyre!("candidate_text is required"))?,
		"source_refs": {
			"social_candidates": [candidate_ref],
		},
		"evidence_notes": candidate.get("evidence_notes").cloned().ok_or_else(|| eyre::eyre!("evidence_notes are required"))?,
		"claims": candidate.get("claims").cloned().ok_or_else(|| eyre::eyre!("claims are required"))?,
		"decision": {
			"worthiness": "skip",
			"priority": required_string(candidate.get("priority"), "priority")?,
			"idempotency_key": required_string(decision.get("idempotency_key"), "idempotency_key")?,
			"reason": reason,
			"daily_limit": 8,
			"daily_count_before": published_count,
			"daily_count_after": published_count,
			"day": day,
			"timezone": timezone,
		},
		"skip": {
			"reason": reason,
		},
	}))
}

fn existing_result(
	root: &Path,
	candidate_path: &Path,
	output_path: &Path,
	expected: &Value,
	idempotency_key: &str,
	published_count: usize,
) -> Result<SocialTerminalizeSkipReport> {
	let existing = crate::load_json(output_path)?;
	if existing != *expected {
		return Err(eyre::eyre!(
			"existing terminal post conflicts with the quality-skip payload: {}",
			crate::path_arg(root, output_path)
		));
	}

	Ok(SocialTerminalizeSkipReport {
		status: "already_skipped".into(),
		path: crate::path_arg(root, output_path),
		candidate: crate::path_arg(root, candidate_path),
		idempotency_key: idempotency_key.into(),
		published_count,
	})
}

fn require_contained_regular_file(path: &Path, root: &Path) -> Result<()> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|error| eyre::eyre!("candidate is unavailable: {error}"))?;
	if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
		return Err(eyre::eyre!("candidate must be a regular non-symlink file"));
	}
	let canonical_root = root
		.canonicalize()
		.map_err(|error| eyre::eyre!("candidate root is unavailable: {error}"))?;
	let canonical_path =
		path.canonicalize().map_err(|error| eyre::eyre!("candidate is unavailable: {error}"))?;
	if !canonical_path.starts_with(&canonical_root) {
		return Err(eyre::eyre!("candidate must stay under the configured candidates directory"));
	}

	Ok(())
}

fn required_string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str> {
	value
		.and_then(Value::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("{field} is required"))
}

fn valid_day(day: &str) -> bool {
	let bytes = day.as_bytes();
	bytes.len() == 10
		&& bytes[4] == b'-'
		&& bytes[7] == b'-'
		&& bytes
			.iter()
			.enumerate()
			.all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}
