use std::path::Path;

use crate::{
	prelude::Result,
	state::{
		RunActivityMarker,
		run_activity_marker::{accounts, storage},
	},
};

pub(crate) fn read_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<Option<i64>> {
	let marker = storage::read_run_activity_marker_record(worktree_path)?.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	Ok(marker.and_then(|marker| marker.last_activity_unix_epoch))
}

pub(crate) fn read_run_protocol_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<Option<i64>> {
	let marker = storage::read_run_activity_marker_record(worktree_path)?.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	Ok(marker.and_then(|marker| marker.last_protocol_activity_unix_epoch))
}

pub(crate) fn read_run_activity_marker_snapshot(
	worktree_path: &Path,
) -> Result<Option<RunActivityMarker>> {
	Ok(storage::read_run_activity_marker_record(worktree_path)?.and_then(|marker| {
		let accounts = accounts::accounts_from_marker_record(&marker);

		Some(RunActivityMarker {
			run_id: marker.run_id?,
			attempt_number: marker.attempt_number?,
			process_id: marker.process_id,
			host_boot_id: marker.host_boot_id,
			process_start_identity: marker.process_start_identity,
			last_activity_unix_epoch: marker.last_activity_unix_epoch,
			last_protocol_activity_unix_epoch: marker.last_protocol_activity_unix_epoch,
			last_progress_unix_epoch: marker.last_progress_unix_epoch,
			current_operation: marker.current_operation,
			thread_id: marker.thread_id,
			turn_id: marker.turn_id,
			thread_status: marker.thread_status,
			thread_active_flags: marker.thread_active_flags,
			event_count: marker.event_count,
			last_event_type: marker.last_event_type,
			effective_model: marker.effective_model,
			effective_model_provider: marker.effective_model_provider,
			effective_cwd: marker.effective_cwd,
			effective_approval_policy: marker.effective_approval_policy,
			effective_approvals_reviewer: marker.effective_approvals_reviewer,
			effective_sandbox_mode: marker.effective_sandbox_mode,
			child_agent_activity: marker.child_agent_activity,
			protocol_activity: marker.protocol_activity,
			account: marker.account,
			accounts,
			retry_budget_attempt_count: marker.retry_budget_attempt_count,
			retry_kind: marker.retry_kind,
			retry_ready_at_unix_epoch: marker.retry_ready_at_unix_epoch,
		})
	}))
}
