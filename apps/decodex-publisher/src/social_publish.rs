//! Social publishing reservation generation and conflict checks.

mod payload;
mod scan;

use crate::{
	SocialReservePublishReport, SocialReservePublishRequest,
	prelude::{Result, eyre},
};

pub(crate) fn reserve_social_publish(
	request: &SocialReservePublishRequest,
) -> Result<SocialReservePublishReport> {
	if request.slug.trim().is_empty() {
		return Err(eyre::eyre!("slug is required"));
	}
	if request.idempotency_key.trim().is_empty() {
		return Err(eyre::eyre!("idempotency_key is required"));
	}
	if request.daily_limit == 0 {
		return Err(eyre::eyre!("daily_limit must be positive"));
	}
	if request.candidate_paths.is_empty() && request.urls.is_empty() {
		return Err(eyre::eyre!("at least one candidate path or URL is required"));
	}
	if request.duplicate_keys.is_empty() {
		return Err(eyre::eyre!("at least one duplicate key is required"));
	}

	let root = crate::repo_root()?;
	let out_dir = crate::resolve_against(&root, &request.out_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let reservation_path =
		out_dir.join(&request.day).join(format!("{}.json", crate::slugify(&request.slug)));
	let scan = scan::scan_social_publish_state(
		&out_dir,
		&posts_dir,
		&request.idempotency_key,
		&request.day,
	)?;

	if scan.idempotency_conflict.is_some() {
		return Err(eyre::eyre!(
			"idempotency_key already has an active reservation or terminal post: {}",
			request.idempotency_key
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

	let payload = payload::social_publish_reservation_payload(request, &root);

	crate::validate_generated_social_artifact(&payload)
		.map_err(|error| eyre::eyre!("generated reservation failed validation: {error}"))?;

	if !request.dry_run {
		crate::write_new_json(&reservation_path, &payload)?;
	}

	Ok(SocialReservePublishReport {
		status: if request.dry_run { "dry_run".into() } else { "reserved".into() },
		path: crate::path_arg(&root, &reservation_path),
		idempotency_key: request.idempotency_key.clone(),
		daily_limit: request.daily_limit,
		published_count: scan.published_count,
		active_reservation_count: scan.active_reservation_count,
	})
}
