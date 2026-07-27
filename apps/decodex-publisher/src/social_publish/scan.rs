#[cfg(unix)] use std::os::unix::fs::MetadataExt as _;
use std::{
	fs::{self, File, OpenOptions},
	path::{Path, PathBuf},
};

use serde_json::Value;

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

pub(crate) fn acquire_social_state_lock(locks_dir: &Path) -> Result<File> {
	let root = crate::repo_root()?;
	let locks_dir = crate::resolve_against(&root, locks_dir);
	fs::create_dir_all(&locks_dir)?;
	let path = locks_dir.join(STATE_MUTATION_LOCK);

	if path.exists() && fs::symlink_metadata(&path)?.file_type().is_symlink() {
		return Err(crate::prelude::eyre::eyre!(
			"social state mutation lock file must not be a symlink"
		));
	}
	let file =
		OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
	let path_metadata = fs::symlink_metadata(&path)?;
	if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
		return Err(crate::prelude::eyre::eyre!(
			"social state mutation lock path must be a regular file"
		));
	}
	#[cfg(unix)]
	{
		let file_metadata = file.metadata()?;
		if file_metadata.dev() != path_metadata.dev() || file_metadata.ino() != path_metadata.ino()
		{
			return Err(crate::prelude::eyre::eyre!(
				"social state mutation lock path changed during open"
			));
		}
	}
	file.lock()?;

	Ok(file)
}

fn existing_json_files(path: &Path) -> Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	crate::collect_json_files(&[path.to_path_buf()])
}
