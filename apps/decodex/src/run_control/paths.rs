use std::{
	fs,
	path::{Path, PathBuf},
	process,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{
	prelude::{Result, eyre},
	run_control::constants::{
		REQUEST_SUFFIX, RESPONSE_SUFFIX, RUN_CONTROL_DIR, STEER_REQUEST_SUFFIX,
		STEER_RESPONSE_SUFFIX,
	},
};

pub(super) fn interrupt_request_path(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		REQUEST_SUFFIX
	))
}

pub(super) fn interrupt_response_path(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		RESPONSE_SUFFIX
	))
}

pub(super) fn steer_request_path(worktree_path: &Path, run_id: &str, request_id: &str) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		STEER_REQUEST_SUFFIX
	))
}

pub(super) fn steer_response_path(worktree_path: &Path, run_id: &str, request_id: &str) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		STEER_RESPONSE_SUFFIX
	))
}

pub(super) fn run_control_run_dir(worktree_path: &Path, run_id: &str) -> PathBuf {
	worktree_path.join(RUN_CONTROL_DIR).join(sanitize_path_component(run_id))
}

pub(super) fn write_json_file_atomically<T>(path: &Path, value: &T) -> Result<()>
where
	T: Serialize,
{
	let parent = path
		.parent()
		.ok_or_else(|| eyre::eyre!("Lane-control file `{}` has no parent.", path.display()))?;
	let temp_path = path.with_extension("tmp");
	let data = serde_json::to_vec_pretty(value)?;

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, data)?;
	fs::rename(&temp_path, path)?;

	Ok(())
}

pub(super) fn fresh_request_id(run_id: &str) -> String {
	let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_nanos();

	format!("{}-{}-{now}", sanitize_path_component(run_id), process::id())
}

pub(super) fn file_name_ends_with(path: &Path, suffix: &str) -> bool {
	path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(suffix))
}

pub(super) fn message_line_count(message: &str) -> usize {
	message.lines().count().max(usize::from(!message.is_empty()))
}

pub(super) fn sanitize_path_component(value: &str) -> String {
	let sanitized = value
		.chars()
		.map(|character| match character {
			'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
			_ => '-',
		})
		.collect::<String>();

	if sanitized.is_empty() { String::from("lane-control") } else { sanitized }
}
