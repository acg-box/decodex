use std::{fs, path::Path};

use crate::{
	prelude::Result,
	state::{
		EffectiveRuntimeMarker, RUN_OPERATION_AGENT_RUN,
		run_activity_marker::{identity, storage},
	},
};

pub(crate) fn write_run_thread_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = Some(thread_id.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_turn_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	turn_id: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.turn_id = Some(turn_id.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_thread_status_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	thread_status: &str,
	thread_active_flags: &[String],
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = turn_id.map(str::to_owned).or(marker.turn_id);
	marker.thread_status = Some(thread_status.to_owned());
	marker.thread_active_flags = thread_active_flags.to_vec();

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_effective_runtime_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	runtime: &EffectiveRuntimeMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = runtime.thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = runtime.turn_id.map(str::to_owned).or(marker.turn_id);
	marker.effective_model = Some(runtime.effective_model.to_owned());
	marker.effective_model_provider = Some(runtime.effective_model_provider.to_owned());
	marker.effective_cwd = Some(runtime.effective_cwd.to_owned());
	marker.effective_approval_policy = Some(runtime.effective_approval_policy.to_owned());
	marker.effective_approvals_reviewer = Some(runtime.effective_approvals_reviewer.to_owned());
	marker.effective_sandbox_mode = Some(runtime.effective_sandbox_mode.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}
