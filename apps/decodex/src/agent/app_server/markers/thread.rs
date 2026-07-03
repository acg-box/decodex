use std::path::Path;

use crate::state;

pub(in crate::agent::app_server) fn write_turn_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	turn_id: &str,
) {
	if let Err(error) = state::write_run_turn_marker(marker_path, run_id, attempt_number, turn_id) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree turn marker."
		);
	}
}

pub(in crate::agent::app_server) fn write_thread_status_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	thread_status: &str,
	thread_active_flags: &[String],
) {
	if let Err(error) = state::write_run_thread_status_marker(
		marker_path,
		run_id,
		attempt_number,
		thread_id,
		turn_id,
		thread_status,
		thread_active_flags,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree thread-status marker."
		);
	}
}

pub(in crate::agent::app_server) fn write_thread_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: &str,
) {
	if let Err(error) =
		state::write_run_thread_marker(marker_path, run_id, attempt_number, thread_id)
	{
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree thread marker."
		);
	}
}
