use std::{fs, path::Path, process};

use time::OffsetDateTime;

use crate::{
	prelude::Result,
	state::run_activity_marker::{identity, record, storage},
};

pub(crate) fn write_run_operation_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	write_run_operation_marker_for_process(
		worktree_path,
		run_id,
		attempt_number,
		process::id(),
		current_operation,
	)
}

pub(crate) fn write_run_operation_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let now = OffsetDateTime::now_utc().unix_timestamp();
	let existing_marker = storage::read_run_activity_marker_record(worktree_path)?;
	let mut marker = record::run_activity_marker_record_for_attempt(
		existing_marker.as_ref(),
		run_id,
		attempt_number,
	);

	identity::set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_progress_unix_epoch = Some(now);
	marker.current_operation = Some(current_operation.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_operation_marker_preserving_activity(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let existing_marker = storage::read_run_activity_marker_record(worktree_path)?;
	let mut marker = record::run_activity_marker_record_for_attempt(
		existing_marker.as_ref(),
		run_id,
		attempt_number,
	);

	marker.current_operation = Some(current_operation.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}
