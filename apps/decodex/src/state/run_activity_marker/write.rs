mod operation;
mod protocol;
mod session;

pub(crate) use self::{
	operation::{
		write_run_operation_marker, write_run_operation_marker_for_process,
		write_run_operation_marker_preserving_activity,
	},
	protocol::{write_run_account_marker, write_run_protocol_activity_marker},
	session::{
		write_run_effective_runtime_marker, write_run_thread_marker,
		write_run_thread_status_marker, write_run_turn_marker,
	},
};

use std::{path::Path, process};

use time::OffsetDateTime;

use crate::{
	prelude::Result,
	state::run_activity_marker::{identity, record, storage},
};

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	write_run_activity_marker_for_process(worktree_path, run_id, attempt_number, process::id())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
) -> Result<()> {
	write_run_activity_marker_at(
		worktree_path,
		run_id,
		attempt_number,
		process_id,
		OffsetDateTime::now_utc().unix_timestamp(),
		None,
	)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_at(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
	last_activity_unix_epoch: i64,
	last_protocol_activity_unix_epoch: Option<i64>,
) -> Result<()> {
	let existing_marker = storage::read_run_activity_marker_record(worktree_path)?;
	let same_run_marker = existing_marker.as_ref().filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});
	let mut marker = record::run_activity_marker_record_for_attempt(
		existing_marker.as_ref(),
		run_id,
		attempt_number,
	);

	identity::set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(last_activity_unix_epoch);
	marker.last_protocol_activity_unix_epoch = last_protocol_activity_unix_epoch
		.or_else(|| same_run_marker.and_then(|marker| marker.last_protocol_activity_unix_epoch));

	if let Some(same_run_marker) = same_run_marker {
		marker.retry_kind = same_run_marker.retry_kind.clone();
		marker.retry_ready_at_unix_epoch = same_run_marker.retry_ready_at_unix_epoch;
	}

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}
