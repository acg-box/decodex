use std::{
	fs::File,
	path::{Path, PathBuf},
};

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA, prelude::Result};

const STATE_MUTATION_LOCK: &str = ".social-state-mutation.lock";

#[derive(Debug, Default)]
pub(crate) struct SocialPublishStateScan {
	pub(crate) published_count: usize,
	pub(crate) active_reservation_count: usize,
	pub(crate) idempotency_conflict: Option<PathBuf>,
}

pub(crate) fn scan_social_publish_state(
	reservations_dir: &Path,
	posts_dir: &Path,
	idempotency_key: &str,
	day: &str,
) -> Result<SocialPublishStateScan> {
	let mut scan = SocialPublishStateScan::default();

	for payload_path in existing_json_files(reservations_dir)? {
		let payload = crate::load_json(&payload_path)?;

		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA)
		{
			continue;
		}
		if payload.get("status").and_then(Value::as_str) == Some("active") {
			if payload.get("day").and_then(Value::as_str) == Some(day) {
				scan.active_reservation_count += 1;
			}
			if payload.get("idempotency_key").and_then(Value::as_str) == Some(idempotency_key) {
				scan.idempotency_conflict.get_or_insert(payload_path);
			}
		}
	}
	for payload_path in existing_json_files(posts_dir)? {
		let payload = crate::load_json(&payload_path)?;

		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA) {
			continue;
		}

		let status = payload.get("status").and_then(Value::as_str);

		if status == Some("published")
			&& payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("day"))
				.and_then(Value::as_str)
				== Some(day)
		{
			scan.published_count += 1;
		}
		if status != Some("failed")
			&& payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("idempotency_key"))
				.and_then(Value::as_str)
				== Some(idempotency_key)
		{
			scan.idempotency_conflict.get_or_insert(payload_path);
		}
	}

	Ok(scan)
}

pub(crate) fn expire_active_reservations(
	reservations_dir: &Path,
	now: OffsetDateTime,
) -> Result<usize> {
	let mut expired_count = 0;
	for payload_path in existing_json_files(reservations_dir)? {
		let payload = crate::load_json(&payload_path)?;
		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA)
			|| payload.get("status").and_then(Value::as_str) != Some("active")
		{
			continue;
		}
		let expires_at = payload
			.get("expires_at")
			.and_then(Value::as_str)
			.ok_or_else(|| crate::prelude::eyre::eyre!("active reservation has no expires_at"))
			.and_then(|value| {
				OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
					crate::prelude::eyre::eyre!("active reservation expires_at is invalid")
				})
			})?;
		if expires_at > now {
			continue;
		}
		let mut expired = payload.clone();
		let object = expired.as_object_mut().ok_or_else(|| {
			crate::prelude::eyre::eyre!("social publish reservation must be an object")
		})?;
		object.insert("status".into(), Value::String("expired".into()));
		object.insert(
			"release_reason".into(),
			Value::String("Reservation expired before publication.".into()),
		);
		crate::validate_generated_social_artifact(&expired)?;
		crate::replace_existing_json(&payload_path, &payload, &expired)?;
		expired_count += 1;
	}

	Ok(expired_count)
}

pub(crate) fn acquire_social_state_lock(locks_dir: &Path) -> Result<File> {
	let root = crate::repo_root()?;
	let locks_dir = crate::resolve_against(&root, locks_dir);
	crate::ensure_private_directory(&locks_dir)?;
	let path = locks_dir.join(STATE_MUTATION_LOCK);
	let file = crate::open_or_create_private_lock(&path)?;
	file.lock()?;

	Ok(file)
}

fn existing_json_files(path: &Path) -> Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	crate::collect_json_files(&[path.to_path_buf()])
}
