mod interrupt;
mod steer;

use crate::{
	agent::app_server::{AppServerClient, AppServerRunRequest, RunRecorder},
	prelude::Result,
	run_control,
};

pub(in crate::agent::app_server) fn handle_pending_turn_control_requests(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	target_thread_id: &str,
	target_turn_id: &str,
) -> Result<Option<String>> {
	let Some(worktree_path) = request.activity_marker_path.as_deref() else {
		return Ok(None);
	};

	for pending in run_control::pending_interrupt_requests(worktree_path, &request.run_id)? {
		interrupt::handle_pending_turn_interrupt_request(
			client,
			recorder,
			request,
			worktree_path,
			pending,
			target_thread_id,
			target_turn_id,
		)?;
	}
	for pending in run_control::pending_steer_requests(worktree_path, &request.run_id)? {
		if let Some(response_turn_id) = steer::handle_pending_turn_steer_request(
			client,
			recorder,
			request,
			worktree_path,
			pending,
			target_thread_id,
			target_turn_id,
		)? {
			return Ok(Some(response_turn_id));
		}
	}

	Ok(None)
}
