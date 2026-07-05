use std::{
	fs,
	path::{Path, PathBuf},
};

use crate::{
	prelude::{self, eyre},
	run_control::{
		constants::{
			REQUEST_SUFFIX, SCHEMA_INTERRUPT_REQUEST, SCHEMA_STEER_REQUEST, STEER_REQUEST_SUFFIX,
		},
		paths,
		types::{
			LaneControlInterruptRequest, LaneControlSteerRequest, PendingLaneControlRequest,
			PendingLaneControlSteerRequest,
		},
	},
};

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
