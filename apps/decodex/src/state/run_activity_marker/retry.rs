use std::{fs, path::Path};

use crate::{
	prelude::Result,
	state::run_activity_marker::{identity, storage},
};

pub(crate) fn write_run_retry_budget_attempt_count(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_budget_attempt_count: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.retry_budget_attempt_count = Some(retry_budget_attempt_count);

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_retry_schedule(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_kind: &str,
	retry_ready_at_unix_epoch: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);
	marker.retry_kind = Some(retry_kind.to_owned());
	marker.retry_ready_at_unix_epoch = Some(retry_ready_at_unix_epoch);

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn clear_run_retry_schedule(worktree_path: &Path) -> Result<()> {
	let Some(mut marker) = storage::read_run_activity_marker_record(worktree_path)? else {
		return Ok(());
	};

	marker.retry_kind = None;
	marker.retry_ready_at_unix_epoch = None;

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn read_run_retry_budget_attempt_count(worktree_path: &Path) -> Result<Option<i64>> {
	Ok(storage::read_run_activity_marker_record(worktree_path)?
		.and_then(|marker| marker.retry_budget_attempt_count))
}
