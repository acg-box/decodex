use std::{
	path::Path,
	thread,
	time::{Duration, Instant},
};

use crate::{
	prelude::Result,
	run_control::{
		constants::POLL_INTERVAL,
		files::read,
		types::{LaneControlInterruptResponse, LaneControlSteerResponse},
	},
};

pub(crate) fn wait_for_interrupt_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
	timeout: Duration,
) -> Result<Option<LaneControlInterruptResponse>> {
	let started_at = Instant::now();

	loop {
		if let Some(response) = read::read_interrupt_response(worktree_path, run_id, request_id)? {
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
) -> Result<Option<LaneControlSteerResponse>> {
	let started_at = Instant::now();

	loop {
		if let Some(response) = read::read_steer_response(worktree_path, run_id, request_id)? {
			return Ok(Some(response));
		}

		if started_at.elapsed() >= timeout {
			return Ok(None);
		}

		thread::sleep(POLL_INTERVAL);
	}
}
