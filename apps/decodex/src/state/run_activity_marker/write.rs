use std::{fs, path::Path, process};

use time::OffsetDateTime;

use crate::{
	prelude::Result,
	state::{
		CodexAccountMarker, EffectiveRuntimeMarker, ProtocolActivityMarker,
		RUN_OPERATION_AGENT_RUN,
		run_activity_marker::{accounts, identity, progress, record, storage},
	},
};

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	self::write_run_activity_marker_for_process(
		worktree_path,
		run_id,
		attempt_number,
		process::id(),
	)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
) -> Result<()> {
	self::write_run_activity_marker_at(
		worktree_path,
		run_id,
		attempt_number,
		process_id,
		OffsetDateTime::now_utc().unix_timestamp(),
		None,
	)
}

pub(crate) fn write_run_operation_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	self::write_run_operation_marker_for_process(
		worktree_path,
		run_id,
		attempt_number,
		process::id(),
		current_operation,
	)
}

pub(crate) fn write_run_operation_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let now = OffsetDateTime::now_utc().unix_timestamp();
	let existing_marker = storage::read_run_activity_marker_record(worktree_path)?;
	let mut marker = record::run_activity_marker_record_for_attempt(
		existing_marker.as_ref(),
		run_id,
		attempt_number,
	);

	identity::set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_progress_unix_epoch = Some(now);
	marker.current_operation = Some(current_operation.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_operation_marker_preserving_activity(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let existing_marker = storage::read_run_activity_marker_record(worktree_path)?;
	let mut marker = record::run_activity_marker_record_for_attempt(
		existing_marker.as_ref(),
		run_id,
		attempt_number,
	);

	marker.current_operation = Some(current_operation.to_owned());

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_protocol_activity_marker(
	worktree_path: &Path,
	activity: &ProtocolActivityMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let now = OffsetDateTime::now_utc().unix_timestamp();
	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(activity.run_id.to_owned());
	marker.attempt_number = Some(activity.attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_protocol_activity_unix_epoch = Some(now);

	if progress::protocol_event_counts_as_work_progress(activity.last_event_type) {
		marker.last_progress_unix_epoch = Some(now);
	}

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = activity.thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = activity.turn_id.map(str::to_owned).or(marker.turn_id);
	marker.event_count = Some(activity.event_count);
	marker.last_event_type = Some(activity.last_event_type.to_owned());
	marker.child_agent_activity = activity.child_agent_activity.cloned();
	marker.protocol_activity = activity.protocol_activity.cloned();

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_account_marker(
	worktree_path: &Path,
	account: &CodexAccountMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = storage::read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(account.run_id.to_owned());
	marker.attempt_number = Some(account.attempt_number);

	identity::ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.account = Some(account.account.clone());
	marker.accounts = accounts::normalize_accounts(account.account, account.accounts);

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

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

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_at(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
	last_activity_unix_epoch: i64,
	last_protocol_activity_unix_epoch: Option<i64>,
) -> Result<()> {
	let existing_marker = storage::read_run_activity_marker_record(worktree_path)?;
	let same_run_marker = existing_marker.as_ref().filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});
	let mut marker = record::run_activity_marker_record_for_attempt(
		existing_marker.as_ref(),
		run_id,
		attempt_number,
	);

	identity::set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(last_activity_unix_epoch);
	marker.last_protocol_activity_unix_epoch = last_protocol_activity_unix_epoch
		.or_else(|| same_run_marker.and_then(|marker| marker.last_protocol_activity_unix_epoch));

	if let Some(same_run_marker) = same_run_marker {
		marker.retry_kind = same_run_marker.retry_kind.clone();
		marker.retry_ready_at_unix_epoch = same_run_marker.retry_ready_at_unix_epoch;
	}

	storage::write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}
