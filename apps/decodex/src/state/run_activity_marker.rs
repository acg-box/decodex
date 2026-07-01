// Runtime activity-marker filesystem helpers.

mod identity;
mod storage;

use std::{fs, path::Path, process};

use time::OffsetDateTime;

use crate::{
	prelude::Result,
	state::{
		ChildAgentActivitySummary, CodexAccountActivitySummary, CodexAccountMarker,
		EffectiveRuntimeMarker, ProtocolActivityMarker, ProtocolActivitySummary,
		RUN_OPERATION_AGENT_RUN, RunActivityMarker,
	},
};

#[cfg(test)] pub(crate) use self::identity::current_process_start_identity;
use self::identity::{
	ensure_run_activity_marker_current_process_identity, set_run_activity_marker_process_identity,
};
pub(crate) use self::{
	identity::{current_host_boot_id, process_start_identity},
	storage::{read_run_activity_marker_record, write_run_activity_marker_record},
};

#[derive(Clone, Default)]
pub(crate) struct RunActivityMarkerRecord {
	run_id: Option<String>,
	attempt_number: Option<i64>,
	process_id: Option<u32>,
	host_boot_id: Option<String>,
	process_start_identity: Option<String>,
	last_activity_unix_epoch: Option<i64>,
	last_protocol_activity_unix_epoch: Option<i64>,
	last_progress_unix_epoch: Option<i64>,
	current_operation: Option<String>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	event_count: Option<i64>,
	last_event_type: Option<String>,
	effective_model: Option<String>,
	effective_model_provider: Option<String>,
	effective_cwd: Option<String>,
	effective_approval_policy: Option<String>,
	effective_approvals_reviewer: Option<String>,
	effective_sandbox_mode: Option<String>,
	child_agent_activity: Option<ChildAgentActivitySummary>,
	protocol_activity: Option<ProtocolActivitySummary>,
	account: Option<CodexAccountActivitySummary>,
	accounts: Vec<CodexAccountActivitySummary>,
	retry_budget_attempt_count: Option<i64>,
	retry_kind: Option<String>,
	retry_ready_at_unix_epoch: Option<i64>,
}

pub(crate) fn protocol_event_counts_as_work_progress(event_type: &str) -> bool {
	let normalized = event_type.to_ascii_lowercase();

	if protocol_event_is_non_work_activity(&normalized) {
		return false;
	}

	normalized.starts_with("turn/")
		|| normalized.starts_with("item/")
		|| normalized == "thread/archive"
		|| normalized.contains("plan")
		|| normalized.contains("diff")
		|| normalized.contains("filechange")
		|| normalized.contains("patch")
		|| normalized.contains("command")
		|| normalized.contains("validation")
		|| normalized.contains("review")
		|| normalized.contains("pull_request")
		|| normalized == "model/response"
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	write_run_activity_marker_for_process(worktree_path, run_id, attempt_number, process::id())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
) -> Result<()> {
	write_run_activity_marker_at(
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
	write_run_operation_marker_for_process(
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
	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let mut marker =
		run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_progress_unix_epoch = Some(now);
	marker.current_operation = Some(current_operation.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_operation_marker_preserving_activity(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let mut marker =
		run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	marker.current_operation = Some(current_operation.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_protocol_activity_marker(
	worktree_path: &Path,
	activity: &ProtocolActivityMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let now = OffsetDateTime::now_utc().unix_timestamp();
	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(activity.run_id.to_owned());
	marker.attempt_number = Some(activity.attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_protocol_activity_unix_epoch = Some(now);

	if protocol_event_counts_as_work_progress(activity.last_event_type) {
		marker.last_progress_unix_epoch = Some(now);
	}

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = activity.thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = activity.turn_id.map(str::to_owned).or(marker.turn_id);
	marker.event_count = Some(activity.event_count);
	marker.last_event_type = Some(activity.last_event_type.to_owned());
	marker.child_agent_activity = activity.child_agent_activity.cloned();
	marker.protocol_activity = activity.protocol_activity.cloned();

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_account_marker(
	worktree_path: &Path,
	account: &CodexAccountMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(account.run_id.to_owned());
	marker.attempt_number = Some(account.attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.account = Some(account.account.clone());
	marker.accounts = normalize_accounts(account.account, account.accounts);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_thread_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = Some(thread_id.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_turn_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	turn_id: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.turn_id = Some(turn_id.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

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

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = turn_id.map(str::to_owned).or(marker.turn_id);
	marker.thread_status = Some(thread_status.to_owned());
	marker.thread_active_flags = thread_active_flags.to_vec();

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_effective_runtime_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	runtime: &EffectiveRuntimeMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = runtime.thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = runtime.turn_id.map(str::to_owned).or(marker.turn_id);
	marker.effective_model = Some(runtime.effective_model.to_owned());
	marker.effective_model_provider = Some(runtime.effective_model_provider.to_owned());
	marker.effective_cwd = Some(runtime.effective_cwd.to_owned());
	marker.effective_approval_policy = Some(runtime.effective_approval_policy.to_owned());
	marker.effective_approvals_reviewer = Some(runtime.effective_approvals_reviewer.to_owned());
	marker.effective_sandbox_mode = Some(runtime.effective_sandbox_mode.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn read_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<Option<i64>> {
	let marker = read_run_activity_marker_record(worktree_path)?.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	Ok(marker.and_then(|marker| marker.last_activity_unix_epoch))
}

pub(crate) fn read_run_protocol_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<Option<i64>> {
	let marker = read_run_activity_marker_record(worktree_path)?.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	Ok(marker.and_then(|marker| marker.last_protocol_activity_unix_epoch))
}

pub(crate) fn write_run_retry_budget_attempt_count(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_budget_attempt_count: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.retry_budget_attempt_count = Some(retry_budget_attempt_count);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_retry_schedule(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_kind: &str,
	retry_ready_at_unix_epoch: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);
	marker.retry_kind = Some(retry_kind.to_owned());
	marker.retry_ready_at_unix_epoch = Some(retry_ready_at_unix_epoch);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn clear_run_retry_schedule(worktree_path: &Path) -> Result<()> {
	let Some(mut marker) = read_run_activity_marker_record(worktree_path)? else {
		return Ok(());
	};

	marker.retry_kind = None;
	marker.retry_ready_at_unix_epoch = None;

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn read_run_retry_budget_attempt_count(worktree_path: &Path) -> Result<Option<i64>> {
	Ok(read_run_activity_marker_record(worktree_path)?
		.and_then(|marker| marker.retry_budget_attempt_count))
}

pub(crate) fn read_run_activity_marker_snapshot(
	worktree_path: &Path,
) -> Result<Option<RunActivityMarker>> {
	Ok(read_run_activity_marker_record(worktree_path)?.and_then(|marker| {
		let accounts = accounts_from_marker_record(&marker);

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

fn protocol_event_is_non_work_activity(normalized_event_type: &str) -> bool {
	normalized_event_type.starts_with("account/")
		|| normalized_event_type.starts_with("skills/")
		|| normalized_event_type.starts_with("thread/goal/")
		|| normalized_event_type.contains("ratelimit")
		|| normalized_event_type.contains("rate_limit")
		|| normalized_event_type == "thread/status/changed"
		|| normalized_event_type.contains("tokenusage")
		|| matches!(
			normalized_event_type,
			"deprecationnotice"
				| "warning" | "configwarning"
				| "guardianwarning"
				| "model/rerouted"
				| "model/verification"
		)
}

fn normalize_accounts(
	selected: &CodexAccountActivitySummary,
	accounts: &[CodexAccountActivitySummary],
) -> Vec<CodexAccountActivitySummary> {
	let mut normalized =
		if accounts.is_empty() { vec![selected.clone()] } else { accounts.to_vec() };

	if !normalized.iter().any(|account| account.account_fingerprint == selected.account_fingerprint)
	{
		normalized.insert(0, selected.clone());
	}

	normalized
}

fn accounts_from_marker_record(
	marker: &RunActivityMarkerRecord,
) -> Vec<CodexAccountActivitySummary> {
	if marker.accounts.is_empty() {
		marker.account.iter().cloned().collect()
	} else {
		marker.accounts.clone()
	}
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
	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let same_run_marker = existing_marker.as_ref().filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});
	let mut marker =
		run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(last_activity_unix_epoch);
	marker.last_protocol_activity_unix_epoch = last_protocol_activity_unix_epoch
		.or_else(|| same_run_marker.and_then(|marker| marker.last_protocol_activity_unix_epoch));

	if let Some(same_run_marker) = same_run_marker {
		marker.retry_kind = same_run_marker.retry_kind.clone();
		marker.retry_ready_at_unix_epoch = same_run_marker.retry_ready_at_unix_epoch;
	}

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

fn run_activity_marker_record_for_attempt(
	existing_marker: Option<&RunActivityMarkerRecord>,
	run_id: &str,
	attempt_number: i64,
) -> RunActivityMarkerRecord {
	let same_run_marker = existing_marker.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	RunActivityMarkerRecord {
		run_id: Some(run_id.to_owned()),
		attempt_number: Some(attempt_number),
		process_id: same_run_marker.and_then(|marker| marker.process_id),
		host_boot_id: same_run_marker.and_then(|marker| marker.host_boot_id.clone()),
		process_start_identity: same_run_marker
			.and_then(|marker| marker.process_start_identity.clone()),
		last_activity_unix_epoch: same_run_marker
			.and_then(|marker| marker.last_activity_unix_epoch),
		last_protocol_activity_unix_epoch: same_run_marker
			.and_then(|marker| marker.last_protocol_activity_unix_epoch),
		last_progress_unix_epoch: same_run_marker
			.and_then(|marker| marker.last_progress_unix_epoch),
		current_operation: same_run_marker.and_then(|marker| marker.current_operation.clone()),
		thread_id: same_run_marker.and_then(|marker| marker.thread_id.clone()),
		turn_id: same_run_marker.and_then(|marker| marker.turn_id.clone()),
		thread_status: same_run_marker.and_then(|marker| marker.thread_status.clone()),
		thread_active_flags: same_run_marker
			.map(|marker| marker.thread_active_flags.clone())
			.unwrap_or_default(),
		event_count: same_run_marker.and_then(|marker| marker.event_count),
		last_event_type: same_run_marker.and_then(|marker| marker.last_event_type.clone()),
		effective_model: same_run_marker.and_then(|marker| marker.effective_model.clone()),
		effective_model_provider: same_run_marker
			.and_then(|marker| marker.effective_model_provider.clone()),
		effective_cwd: same_run_marker.and_then(|marker| marker.effective_cwd.clone()),
		effective_approval_policy: same_run_marker
			.and_then(|marker| marker.effective_approval_policy.clone()),
		effective_approvals_reviewer: same_run_marker
			.and_then(|marker| marker.effective_approvals_reviewer.clone()),
		effective_sandbox_mode: same_run_marker
			.and_then(|marker| marker.effective_sandbox_mode.clone()),
		child_agent_activity: same_run_marker
			.and_then(|marker| marker.child_agent_activity.clone()),
		protocol_activity: same_run_marker.and_then(|marker| marker.protocol_activity.clone()),
		account: same_run_marker.and_then(|marker| marker.account.clone()),
		accounts: same_run_marker.map(|marker| marker.accounts.clone()).unwrap_or_default(),
		retry_budget_attempt_count: existing_marker
			.and_then(|marker| marker.retry_budget_attempt_count),
		retry_kind: same_run_marker.and_then(|marker| marker.retry_kind.clone()),
		retry_ready_at_unix_epoch: same_run_marker
			.and_then(|marker| marker.retry_ready_at_unix_epoch),
	}
}
