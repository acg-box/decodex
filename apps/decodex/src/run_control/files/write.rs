use std::path::{Path, PathBuf};

use crate::{
	prelude::Result,
	run_control::{
		paths,
		types::{
			LaneControlInterruptRequest, LaneControlInterruptResponse, LaneControlSteerRequest,
			LaneControlSteerResponse,
		},
	},
};

pub(crate) fn write_interrupt_request(
	worktree_path: &Path,
	request: &LaneControlInterruptRequest,
) -> Result<PathBuf> {
	let path = paths::interrupt_request_path(worktree_path, &request.run_id, &request.request_id);

	paths::write_json_file_atomically(&path, request)?;

	Ok(path)
}

pub(crate) fn write_interrupt_response(
	worktree_path: &Path,
	response: &LaneControlInterruptResponse,
) -> Result<PathBuf> {
	let path =
		paths::interrupt_response_path(worktree_path, &response.run_id, &response.request_id);

	paths::write_json_file_atomically(&path, response)?;

	Ok(path)
}

pub(crate) fn write_steer_request(
	worktree_path: &Path,
	request: &LaneControlSteerRequest,
) -> Result<PathBuf> {
	let path = paths::steer_request_path(worktree_path, &request.run_id, &request.request_id);

	paths::write_json_file_atomically(&path, request)?;

	Ok(path)
}

pub(crate) fn write_steer_response(
	worktree_path: &Path,
	response: &LaneControlSteerResponse,
) -> Result<PathBuf> {
	let path = paths::steer_response_path(worktree_path, &response.run_id, &response.request_id);

	paths::write_json_file_atomically(&path, response)?;

	Ok(path)
}
