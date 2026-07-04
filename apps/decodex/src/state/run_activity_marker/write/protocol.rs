use std::{fs, path::Path};

use time::OffsetDateTime;

use crate::{
	prelude::Result,
	state::{
		CodexAccountMarker, ProtocolActivityMarker, RUN_OPERATION_AGENT_RUN,
		run_activity_marker::{accounts, identity, progress, storage},
	},
};

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
