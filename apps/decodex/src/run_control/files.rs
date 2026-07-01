use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	thread,
	time::{Duration, Instant},
};

use crate::{
	prelude::{self, eyre},
	run_control::{
		constants::{
			POLL_INTERVAL, REQUEST_SUFFIX, SCHEMA_INTERRUPT_REQUEST, SCHEMA_STEER_REQUEST,
			STEER_REQUEST_SUFFIX,
		},
		paths,
		types::{
			LaneControlInterruptRequest, LaneControlInterruptResponse, LaneControlSteerRequest,
			LaneControlSteerResponse, PendingLaneControlRequest, PendingLaneControlSteerRequest,
		},
	},
};

pub(crate) fn write_interrupt_request(
	worktree_path: &Path,
	request: &LaneControlInterruptRequest,
) -> prelude::Result<PathBuf> {
	let path = paths::interrupt_request_path(worktree_path, &request.run_id, &request.request_id);

	paths::write_json_file_atomically(&path, request)?;

	Ok(path)
}

pub(crate) fn write_interrupt_response(
	worktree_path: &Path,
	response: &LaneControlInterruptResponse,
) -> prelude::Result<PathBuf> {
	let path =
		paths::interrupt_response_path(worktree_path, &response.run_id, &response.request_id);

	paths::write_json_file_atomically(&path, response)?;

	Ok(path)
}

pub(crate) fn write_steer_request(
	worktree_path: &Path,
	request: &LaneControlSteerRequest,
) -> prelude::Result<PathBuf> {
	let path = paths::steer_request_path(worktree_path, &request.run_id, &request.request_id);

	paths::write_json_file_atomically(&path, request)?;

	Ok(path)
}

pub(crate) fn write_steer_response(
	worktree_path: &Path,
	response: &LaneControlSteerResponse,
) -> prelude::Result<PathBuf> {
	let path = paths::steer_response_path(worktree_path, &response.run_id, &response.request_id);

	paths::write_json_file_atomically(&path, response)?;

	Ok(path)
}

pub(crate) fn remove_interrupt_request(path: &Path) -> prelude::Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn remove_steer_request(path: &Path) -> prelude::Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn pending_interrupt_requests(
	worktree_path: &Path,
	run_id: &str,
) -> prelude::Result<Vec<PendingLaneControlRequest>> {
	let dir = paths::run_control_run_dir(worktree_path, run_id);
	let Ok(entries) = fs::read_dir(&dir) else {
		return Ok(Vec::new());
	};
	let mut requests = entries
		.filter_map(std::result::Result::ok)
		.map(|entry| entry.path())
		.filter(|path| paths::file_name_ends_with(path, REQUEST_SUFFIX))
		.map(read_pending_interrupt_request)
		.collect::<prelude::Result<Vec<_>>>()?;

	requests.sort_by(|left, right| {
		left.request
			.created_at_unix_epoch
			.cmp(&right.request.created_at_unix_epoch)
			.then_with(|| left.request.request_id.cmp(&right.request.request_id))
	});

	Ok(requests)
}

pub(crate) fn pending_steer_requests(
	worktree_path: &Path,
	run_id: &str,
) -> prelude::Result<Vec<PendingLaneControlSteerRequest>> {
	let dir = paths::run_control_run_dir(worktree_path, run_id);
	let Ok(entries) = fs::read_dir(&dir) else {
		return Ok(Vec::new());
	};
	let mut requests = entries
		.filter_map(std::result::Result::ok)
		.map(|entry| entry.path())
		.filter(|path| paths::file_name_ends_with(path, STEER_REQUEST_SUFFIX))
		.map(read_pending_steer_request)
		.collect::<prelude::Result<Vec<_>>>()?;

	requests.sort_by(|left, right| {
		left.request
			.created_at_unix_epoch
			.cmp(&right.request.created_at_unix_epoch)
			.then_with(|| left.request.request_id.cmp(&right.request.request_id))
	});

	Ok(requests)
}

pub(crate) fn wait_for_interrupt_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
	timeout: Duration,
) -> prelude::Result<Option<LaneControlInterruptResponse>> {
	let started_at = Instant::now();

	loop {
		if let Some(response) = read_interrupt_response(worktree_path, run_id, request_id)? {
			return Ok(Some(response));
		}

		if started_at.elapsed() >= timeout {
			return Ok(None);
		}

		thread::sleep(POLL_INTERVAL);
	}
}

pub(crate) fn wait_for_steer_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
	timeout: Duration,
) -> prelude::Result<Option<LaneControlSteerResponse>> {
	let started_at = Instant::now();

	loop {
		if let Some(response) = read_steer_response(worktree_path, run_id, request_id)? {
			return Ok(Some(response));
		}

		if started_at.elapsed() >= timeout {
			return Ok(None);
		}

		thread::sleep(POLL_INTERVAL);
	}
}

pub(crate) fn read_interrupt_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> prelude::Result<Option<LaneControlInterruptResponse>> {
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
) -> prelude::Result<Option<LaneControlSteerResponse>> {
	let path = paths::steer_response_path(worktree_path, run_id, request_id);

	match fs::read_to_string(path) {
		Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(Into::into),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

fn read_pending_interrupt_request(path: PathBuf) -> prelude::Result<PendingLaneControlRequest> {
	let raw = fs::read_to_string(&path)?;
	let request: LaneControlInterruptRequest = serde_json::from_str(&raw)?;

	if request.schema != SCHEMA_INTERRUPT_REQUEST {
		eyre::bail!(
			"Unsupported lane-control request schema `{}` in `{}`.",
			request.schema,
			path.display()
		);
	}

	Ok(PendingLaneControlRequest { path, request })
}

fn read_pending_steer_request(path: PathBuf) -> prelude::Result<PendingLaneControlSteerRequest> {
	let raw = fs::read_to_string(&path)?;
	let request: LaneControlSteerRequest = serde_json::from_str(&raw)?;

	if request.schema != SCHEMA_STEER_REQUEST {
		eyre::bail!(
			"Unsupported lane-control steer request schema `{}` in `{}`.",
			request.schema,
			path.display()
		);
	}

	Ok(PendingLaneControlSteerRequest { path, request })
}
