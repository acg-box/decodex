//! Social publishing reservation generation and conflict checks.

mod payload;
pub(crate) mod scan;

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
	crate::social_evidence::validate_internal_evidence_files(&candidate)
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
