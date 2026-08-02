//! Social publishing reservation generation and conflict checks.

mod payload;
pub(crate) mod scan;

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SocialReservePublishReport, SocialReservePublishRequest,
	prelude::{Result, eyre},
};

pub(crate) fn reserve_social_publish(
	request: &SocialReservePublishRequest,
) -> Result<SocialReservePublishReport> {
	if request.daily_limit != 1 {
		return Err(eyre::eyre!("daily_limit must be 1"));
	}
	if !valid_run_id(&request.run_id) {
		return Err(eyre::eyre!("run_id must be a lowercase UUID"));
	}
	let reserved_at = OffsetDateTime::parse(&request.reserved_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("reserved_at must be an RFC3339 timestamp"))?;
	let expires_at = OffsetDateTime::parse(&request.expires_at, &Rfc3339)
		.map_err(|_| eyre::eyre!("expires_at must be an RFC3339 timestamp"))?;
	if expires_at <= reserved_at {
		return Err(eyre::eyre!("expires_at must be later than reserved_at"));
	}

	let root = crate::repo_root()?;
	let candidates_dir = crate::resolve_against(&root, &request.candidates_dir);
	let candidate_path = crate::resolve_against(&root, &request.candidate_path);
	crate::require_contained_regular_file(&candidate_path, &candidates_dir)
		.map_err(|error| eyre::eyre!("candidate is invalid: {error}"))?;
	let (candidate, _) = crate::load_json_with_sha256(&candidate_path)?;
	crate::validate_generated_social_artifact(&candidate)
		.map_err(|error| eyre::eyre!("candidate failed validation: {error}"))?;
	crate::social_evidence::validate_source_evidence(&candidate)
		.map_err(|error| eyre::eyre!("candidate evidence failed validation: {error}"))?;
	if candidate.get("schema").and_then(serde_json::Value::as_str)
		!= Some(crate::SOCIAL_CANDIDATE_SCHEMA)
	{
		return Err(eyre::eyre!("candidate must use {}", crate::SOCIAL_CANDIDATE_SCHEMA));
	}
	let decision = candidate
		.get("decision")
		.and_then(serde_json::Value::as_object)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	if decision.get("worthiness").and_then(serde_json::Value::as_str) != Some("publish") {
		return Err(eyre::eyre!("candidate decision.worthiness must be publish"));
	}
	let idempotency_key = decision
		.get("idempotency_key")
		.and_then(serde_json::Value::as_str)
		.ok_or_else(|| eyre::eyre!("candidate idempotency_key is required"))?;
	let publication_lineage_sha256 = crate::social_record::publication_lineage_sha256(&candidate)?;
	let out_dir = crate::resolve_against(&root, &request.out_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	let _state_lock = scan::acquire_social_state_lock(&request.locks_dir)?;
	scan::expire_active_reservations(&out_dir, reserved_at)?;
	if let Some(conflict) = crate::social_xurl::publication_effect_conflict(
		&attempts_dir,
		&publication_lineage_sha256,
		None,
	)? {
		return Err(eyre::eyre!(
			"candidate has a prior uncertain or verified public-write attempt: {}",
			crate::path_arg(&root, &conflict)
		));
	}
	if let Some(conflict) =
		crate::social_xurl::daily_publication_effect_conflict(&attempts_dir, &request.day)?
	{
		return Err(eyre::eyre!(
			"daily public-write cap is already consumed for {}: {}",
			request.day,
			crate::path_arg(&root, &conflict)
		));
	}
	let reservation_path =
		out_dir.join(&request.day).join(format!("{}.json", idempotency_digest(idempotency_key)));
	let scan =
		scan::scan_social_publish_state(&out_dir, &posts_dir, idempotency_key, &request.day)?;

	if scan.idempotency_conflict.is_some() {
		return Err(eyre::eyre!(
			"idempotency_key already has an active reservation or terminal post: {}",
			idempotency_key
		));
	}
	if scan.published_count + scan.active_reservation_count >= request.daily_limit {
		return Err(eyre::eyre!(
			"daily publish cap exhausted for {}: published={}, active_reservations={}, limit={}",
			request.day,
			scan.published_count,
			scan.active_reservation_count,
			request.daily_limit
		));
	}
	let reservation_path = reservation_path_for_write(
		&root,
		&out_dir,
		&attempts_dir,
		&reservation_path,
		&candidate_path,
		idempotency_key,
		&publication_lineage_sha256,
		request,
	)?;

	let payload =
		payload::social_publish_reservation_payload(request, &root, &candidate, &candidate_path)?;

	crate::validate_generated_social_artifact(&payload)
		.map_err(|error| eyre::eyre!("generated reservation failed validation: {error}"))?;

	if !request.dry_run {
		crate::write_new_json(&reservation_path, &payload)?;
	}

	Ok(SocialReservePublishReport {
		status: if request.dry_run { "dry_run".into() } else { "reserved".into() },
		path: crate::path_arg(&root, &reservation_path),
		idempotency_key: idempotency_key.into(),
		daily_limit: request.daily_limit,
		published_count: scan.published_count,
		active_reservation_count: scan.active_reservation_count,
	})
}

pub(crate) fn release_orphaned_active_reservation(
	reservation_path: &std::path::Path,
	reservations_dir: &std::path::Path,
	attempts_dir: &std::path::Path,
	locks_dir: &std::path::Path,
	replacement_run_id: &str,
) -> Result<bool> {
	if !valid_run_id(replacement_run_id) {
		return Err(eyre::eyre!("replacement run_id must be a lowercase UUID"));
	}
	let root = crate::repo_root()?;
	let reservations_dir = crate::resolve_against(&root, reservations_dir);
	let attempts_dir = crate::resolve_against(&root, attempts_dir);
	let reservation_path = crate::resolve_against(&root, reservation_path);
	let _state_lock = scan::acquire_social_state_lock(locks_dir)?;
	crate::require_contained_regular_file(&reservation_path, &reservations_dir)
		.map_err(|error| eyre::eyre!("orphaned reservation is invalid: {error}"))?;
	let reservation = crate::load_json(&reservation_path)?;
	crate::validate_generated_social_artifact(&reservation)
		.map_err(|error| eyre::eyre!("orphaned reservation failed validation: {error}"))?;
	if reservation.get("schema").and_then(Value::as_str)
		!= Some(crate::SOCIAL_PUBLISH_RESERVATION_SCHEMA)
	{
		return Err(eyre::eyre!("orphaned reservation uses an unsupported schema"));
	}
	if reservation.get("status").and_then(Value::as_str) != Some("active") {
		return Ok(false);
	}
	let owner_run_id = reservation
		.pointer("/owner/run_id")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("orphaned reservation owner is missing"))?;
	if !valid_run_id(owner_run_id) {
		return Err(eyre::eyre!("orphaned reservation owner is invalid"));
	}
	if owner_run_id == replacement_run_id {
		return Ok(false);
	}

	let reservation_ref = crate::path_arg(&root, &reservation_path);
	for path in crate::collect_json_files(&[attempts_dir])? {
		let payload = crate::load_json(&path)?;
		match payload.get("schema").and_then(Value::as_str) {
			Some(crate::social_xurl::model::ATTEMPT_SCHEMA) => {
				let attempt: crate::social_xurl::model::XurlAttempt =
					serde_json::from_value(payload).map_err(|_| {
						eyre::eyre!("{} is not a valid xurl publication attempt", path.display())
					})?;
				crate::social_xurl::ledger::validate_publication_cost_record(&attempt)?;
				if attempt.reservation_ref == reservation_ref {
					return Err(eyre::eyre!(
						"publication reservation has a durable xurl attempt and cannot be owner-released"
					));
				}
			},
			Some(crate::social_xurl::model::OBSERVATION_ATTEMPT_SCHEMA) => {
				let attempt: crate::social_xurl::model::XurlObservationAttempt =
					serde_json::from_value(payload).map_err(|_| {
						eyre::eyre!("{} is not a valid xurl observation attempt", path.display())
					})?;
				crate::social_xurl::ledger::validate_observation_cost_record(&attempt)?;
			},
			_ => return Err(eyre::eyre!("{} has invalid xurl attempt state", path.display())),
		}
	}

	let mut released = reservation.clone();
	let object = released
		.as_object_mut()
		.ok_or_else(|| eyre::eyre!("publication reservation must be an object"))?;
	object.insert("status".into(), Value::String("expired".into()));
	object.insert(
		"release_reason".into(),
		Value::String("Reservation owner ended before any durable xurl attempt.".into()),
	);
	crate::validate_generated_social_artifact(&released)?;
	crate::replace_existing_json(&reservation_path, &reservation, &released)?;
	Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn reservation_path_for_write(
	root: &std::path::Path,
	reservations_dir: &std::path::Path,
	attempts_dir: &std::path::Path,
	default_path: &std::path::Path,
	candidate_path: &std::path::Path,
	idempotency_key: &str,
	publication_lineage_sha256: &str,
	request: &SocialReservePublishRequest,
) -> Result<std::path::PathBuf> {
	match std::fs::symlink_metadata(default_path) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
			return Ok(default_path.to_path_buf());
		},
		Err(error) => return Err(error.into()),
		Ok(_) => {},
	}
	crate::require_contained_regular_file(default_path, reservations_dir)
		.map_err(|error| eyre::eyre!("prior reservation is invalid: {error}"))?;
	let prior = crate::load_json(default_path)?;
	crate::validate_generated_social_artifact(&prior)
		.map_err(|error| eyre::eyre!("prior reservation failed validation: {error}"))?;
	let prior_run_id = prior
		.pointer("/owner/run_id")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("prior reservation owner is missing"))?;
	if prior.get("schema").and_then(Value::as_str) != Some(crate::SOCIAL_PUBLISH_RESERVATION_SCHEMA)
		|| prior.get("idempotency_key").and_then(Value::as_str) != Some(idempotency_key)
		|| prior.get("publication_lineage_sha256").and_then(Value::as_str)
			!= Some(publication_lineage_sha256)
		|| prior.get("day").and_then(Value::as_str) != Some(&request.day)
		|| prior.pointer("/candidate_refs/social_candidates/0").and_then(Value::as_str)
			!= Some(crate::path_arg(root, candidate_path).as_str())
		|| !valid_run_id(prior_run_id)
	{
		return Err(eyre::eyre!(
			"existing deterministic reservation is not the same publication lineage"
		));
	}

	let prior_reservation_ref = crate::path_arg(root, default_path);
	let mut attempts = Vec::new();
	for path in crate::collect_json_files(&[attempts_dir.to_path_buf()])? {
		let attempt = crate::load_json(&path)?;
		if attempt.get("schema").and_then(Value::as_str)
			== Some(crate::social_xurl::model::ATTEMPT_SCHEMA)
			&& attempt.get("run_id").and_then(Value::as_str) == Some(prior_run_id)
			&& attempt.get("reservation_ref").and_then(Value::as_str)
				== Some(prior_reservation_ref.as_str())
		{
			attempts.push(path);
		}
	}
	let released_without_attempt = attempts.is_empty()
		&& matches!(prior.get("status").and_then(Value::as_str), Some("expired" | "canceled"));
	let terminal_no_create_attempt = attempts.len() == 1
		&& crate::social_xurl::terminal_no_create_recovery(
			&attempts[0],
			attempts_dir,
			reservations_dir,
		)?;
	if !released_without_attempt && !terminal_no_create_attempt {
		return Err(eyre::eyre!(
			"existing deterministic reservation is not a terminal no-create recovery"
		));
	}

	let retry_path = default_path.with_file_name(format!(
		"{}-{}.json",
		idempotency_digest(idempotency_key),
		request.run_id
	));
	match std::fs::symlink_metadata(&retry_path) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(retry_path),
		Err(error) => Err(error.into()),
		Ok(_) => Err(eyre::eyre!("fresh identity-recovery reservation already exists")),
	}
}

pub(crate) fn idempotency_digest(idempotency_key: &str) -> String {
	Sha256::digest(idempotency_key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn valid_run_id(value: &str) -> bool {
	let bytes = value.as_bytes();
	bytes.len() == 36
		&& matches!(bytes.get(8), Some(b'-'))
		&& matches!(bytes.get(13), Some(b'-'))
		&& matches!(bytes.get(18), Some(b'-'))
		&& matches!(bytes.get(23), Some(b'-'))
		&& bytes.iter().enumerate().all(|(index, byte)| {
			matches!(index, 8 | 13 | 18 | 23)
				|| byte.is_ascii_digit()
				|| matches!(byte, b'a'..=b'f')
		})
}
