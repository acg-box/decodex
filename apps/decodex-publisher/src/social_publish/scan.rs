use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA, prelude::Result};

#[derive(Debug, Default)]
pub(super) struct SocialPublishStateScan {
	pub(super) published_count: usize,
	pub(super) active_reservation_count: usize,
	pub(super) idempotency_conflict: Option<PathBuf>,
}

pub(super) fn scan_social_publish_state(
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

fn existing_json_files(path: &Path) -> Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	crate::collect_json_files(&[path.to_path_buf()])
}
