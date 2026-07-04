mod account_preservation;
mod atomic_write;
mod parse;
mod serialize;

use std::{fs, io::ErrorKind, path::Path};

use crate::{
	prelude::Result,
	state::{RUN_ACTIVITY_MARKER_FILE, run_activity_marker::record::RunActivityMarkerRecord},
};

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn read_run_activity_marker_record(
	worktree_path: &Path,
) -> Result<Option<RunActivityMarkerRecord>> {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = match fs::read_to_string(&marker_path) {
		Ok(body) => body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.into()),
	};

	Ok(Some(parse::parse_run_activity_marker_record(&marker_body)))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_record(
	worktree_path: &Path,
	marker: &RunActivityMarkerRecord,
) -> Result<()> {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let mut marker = marker.clone();

	if let Some(current_marker) = read_run_activity_marker_record(worktree_path)? {
		account_preservation::preserve_current_run_account_marker_fields(
			&current_marker,
			&mut marker,
		);
	}

	atomic_write::write_run_activity_marker_body_atomic(
		&marker_path,
		&serialize::serialize_run_activity_marker_record(&marker),
	)?;

	Ok(())
}
