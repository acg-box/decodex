use std::{fs, io::ErrorKind, path::Path};

use crate::{
	prelude::Result,
	run_control::{
		paths,
		types::{LaneControlInterruptResponse, LaneControlSteerResponse},
	},
};

pub(crate) fn read_interrupt_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> Result<Option<LaneControlInterruptResponse>> {
	let path = paths::interrupt_response_path(worktree_path, run_id, request_id);

	match fs::read_to_string(path) {
		Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(Into::into),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn read_steer_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> Result<Option<LaneControlSteerResponse>> {
	let path = paths::steer_response_path(worktree_path, run_id, request_id);

	match fs::read_to_string(path) {
		Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(Into::into),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}
