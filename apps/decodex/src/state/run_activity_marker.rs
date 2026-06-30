// Runtime activity-marker filesystem helpers.

use std::{
	fs::{self, OpenOptions},
	io::{ErrorKind, Write},
	path::Path,
	process,
	sync::{OnceLock, atomic::AtomicU64},
};

#[cfg(target_os = "macos")]
use std::mem::{self, MaybeUninit};
#[cfg(target_os = "macos")]
use std::ptr;

#[cfg(target_os = "macos")]
use libc::{PROC_PIDTBSDINFO, c_char, c_void, proc_bsdinfo};
use time::OffsetDateTime;

use crate::{
	prelude::{Result, eyre},
	state::{
		ChildAgentActivitySummary, CodexAccountActivitySummary, CodexAccountMarker,
		EffectiveRuntimeMarker, ProtocolActivityMarker, ProtocolActivitySummary,
		RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_AGENT_RUN, RunActivityMarker,
	},
};

static RUN_ACTIVITY_MARKER_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

pub(crate) fn current_host_boot_id() -> Option<String> {
	static CURRENT_HOST_BOOT_ID: OnceLock<Option<String>> = OnceLock::new();

	CURRENT_HOST_BOOT_ID.get_or_init(read_current_host_boot_id).clone()
}

pub(crate) fn current_process_start_identity() -> Option<String> {
	static CURRENT_PROCESS_START_IDENTITY: OnceLock<Option<String>> = OnceLock::new();

	CURRENT_PROCESS_START_IDENTITY.get_or_init(|| process_start_identity(process::id())).clone()
}

pub(crate) fn process_start_identity(process_id: u32) -> Option<String> {
	read_platform_process_start_identity(process_id)
		.and_then(|identity| normalized_process_start_identity(&identity))
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

fn set_run_activity_marker_process_identity(marker: &mut RunActivityMarkerRecord, process_id: u32) {
	marker.process_id = Some(process_id);
	marker.host_boot_id = current_host_boot_id();
	marker.process_start_identity = if process_id == process::id() {
		current_process_start_identity()
	} else {
		process_start_identity(process_id)
	};
}

fn ensure_run_activity_marker_current_process_identity(marker: &mut RunActivityMarkerRecord) {
	let current_process_id = process::id();

	match marker.process_id {
		None => set_run_activity_marker_process_identity(marker, current_process_id),
		Some(process_id)
			if process_id == current_process_id
				&& (marker.host_boot_id.is_none() || marker.process_start_identity.is_none()) =>
		{
			if marker.host_boot_id.is_none() {
				marker.host_boot_id = current_host_boot_id();
			}
			if marker.process_start_identity.is_none() {
				marker.process_start_identity = current_process_start_identity();
			}
		},
		Some(_) => {},
	}
}

fn read_current_host_boot_id() -> Option<String> {
	read_platform_host_boot_id().and_then(|boot_id| normalized_host_boot_id(&boot_id))
}

#[cfg(target_os = "linux")]
fn read_platform_host_boot_id() -> Option<String> {
	fs::read_to_string("/proc/sys/kernel/random/boot_id")
		.ok()
		.map(|boot_id| format!("linux:{boot_id}"))
}

#[cfg(target_os = "macos")]
fn read_platform_host_boot_id() -> Option<String> {
	const BOOT_SESSION_UUID_SYSCTL: &[u8] = b"kern.bootsessionuuid\0";

	let mut size = 0_usize;
	let query_status = unsafe {
		libc::sysctlbyname(
			BOOT_SESSION_UUID_SYSCTL.as_ptr().cast::<c_char>(),
			ptr::null_mut(),
			&mut size,
			ptr::null_mut(),
			0,
		)
	};

	if query_status != 0 || size == 0 {
		return None;
	}

	let mut boot_id = vec![0_u8; size];
	let read_status = unsafe {
		libc::sysctlbyname(
			BOOT_SESSION_UUID_SYSCTL.as_ptr().cast::<c_char>(),
			boot_id.as_mut_ptr().cast::<c_void>(),
			&mut size,
			ptr::null_mut(),
			0,
		)
	};

	if read_status != 0 || size == 0 {
		return None;
	}

	boot_id.truncate(size);

	if boot_id.last() == Some(&0) {
		boot_id.pop();
	}

	String::from_utf8(boot_id).ok().map(|boot_id| format!("macos_bootsessionuuid:{boot_id}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_platform_host_boot_id() -> Option<String> {
	None
}

fn normalized_host_boot_id(boot_id: &str) -> Option<String> {
	let normalized = boot_id.split_whitespace().collect::<Vec<_>>().join(" ");

	(!normalized.is_empty()).then_some(normalized)
}

#[cfg(target_os = "linux")]
fn read_platform_process_start_identity(process_id: u32) -> Option<String> {
	let stat = fs::read_to_string(format!("/proc/{process_id}/stat")).ok()?;
	let comm_end = stat.rfind(')')?;
	let after_comm = stat.get(comm_end + 2..)?;
	let start_time = after_comm.split_whitespace().nth(19)?;

	Some(format!("linux_starttime:{start_time}"))
}

#[cfg(target_os = "macos")]
fn read_platform_process_start_identity(process_id: u32) -> Option<String> {
	let Ok(pid) = i32::try_from(process_id) else {
		return None;
	};

	if pid <= 0 {
		return None;
	}

	let mut info = MaybeUninit::<proc_bsdinfo>::zeroed();
	let Ok(info_size) = i32::try_from(mem::size_of::<proc_bsdinfo>()) else {
		return None;
	};
	let read_size = unsafe {
		libc::proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast::<c_void>(), info_size)
	};

	if read_size != info_size {
		return None;
	}

	let info = unsafe { info.assume_init() };

	Some(format!("macos_starttime:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_platform_process_start_identity(_process_id: u32) -> Option<String> {
	None
}

fn normalized_process_start_identity(identity: &str) -> Option<String> {
	let normalized = identity.split_whitespace().collect::<Vec<_>>().join(" ");

	(!normalized.is_empty()).then_some(normalized)
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

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn read_run_activity_marker_record(
	worktree_path: &Path,
) -> Result<Option<RunActivityMarkerRecord>> {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = match fs::read_to_string(&marker_path) {
		Ok(body) => body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.into()),
	};
	let mut marker = RunActivityMarkerRecord::default();

	for line in marker_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"run_id" => marker.run_id = Some(value.to_owned()),
			"attempt_number" => marker.attempt_number = value.parse::<i64>().ok(),
			"process_id" => marker.process_id = value.parse::<u32>().ok(),
			"host_boot_id" => marker.host_boot_id = Some(value.to_owned()),
			"process_start_identity" => marker.process_start_identity = Some(value.to_owned()),
			"last_activity_unix_epoch" => {
				marker.last_activity_unix_epoch = value.parse::<i64>().ok()
			},
			"last_protocol_activity_unix_epoch" => {
				marker.last_protocol_activity_unix_epoch = value.parse::<i64>().ok()
			},
			"last_progress_unix_epoch" => {
				marker.last_progress_unix_epoch = value.parse::<i64>().ok()
			},
			"current_operation" => marker.current_operation = Some(value.to_owned()),
			"thread_id" => marker.thread_id = Some(value.to_owned()),
			"turn_id" => marker.turn_id = Some(value.to_owned()),
			"thread_status" => marker.thread_status = Some(value.to_owned()),
			"thread_active_flags" => marker.thread_active_flags = parse_marker_list(value),
			"event_count" => marker.event_count = value.parse::<i64>().ok(),
			"last_event_type" => marker.last_event_type = Some(value.to_owned()),
			"effective_model" => marker.effective_model = Some(value.to_owned()),
			"effective_model_provider" => marker.effective_model_provider = Some(value.to_owned()),
			"effective_cwd" => marker.effective_cwd = Some(value.to_owned()),
			"effective_approval_policy" => {
				marker.effective_approval_policy = Some(value.to_owned())
			},
			"effective_approvals_reviewer" => {
				marker.effective_approvals_reviewer = Some(value.to_owned())
			},
			"effective_sandbox_mode" => marker.effective_sandbox_mode = Some(value.to_owned()),
			"child_agent_activity" => {
				marker.child_agent_activity = serde_json::from_str(value).ok()
			},
			"protocol_activity" => marker.protocol_activity = serde_json::from_str(value).ok(),
			"account" => marker.account = serde_json::from_str(value).ok(),
			"accounts" => {
				if let Ok(accounts) = serde_json::from_str(value) {
					marker.accounts = accounts;
				}
			},
			"retry_budget_attempt_count" => {
				marker.retry_budget_attempt_count = value.parse::<i64>().ok()
			},
			"retry_kind" => marker.retry_kind = Some(value.to_owned()),
			"retry_ready_at_unix_epoch" => {
				marker.retry_ready_at_unix_epoch = value.parse::<i64>().ok()
			},
			_ => {},
		}
	}

	Ok(Some(marker))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_record(
	worktree_path: &Path,
	marker: &RunActivityMarkerRecord,
) -> Result<()> {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let mut marker = marker.clone();

	if let Some(current_marker) = read_run_activity_marker_record(worktree_path)? {
		preserve_current_run_account_marker_fields(&current_marker, &mut marker);
	}

	write_run_activity_marker_body_atomic(
		&marker_path,
		&serialize_run_activity_marker_record(&marker),
	)?;

	Ok(())
}

fn preserve_current_run_account_marker_fields(
	current: &RunActivityMarkerRecord,
	next: &mut RunActivityMarkerRecord,
) {
	if current.run_id != next.run_id || current.attempt_number != next.attempt_number {
		return;
	}

	let Some(current_account) = selected_marker_account(current).cloned() else {
		return;
	};
	let keep_current_account = match next.account.as_ref() {
		Some(next_account) => {
			account_marker_observed_unix_epoch(&current_account)
				> account_marker_observed_unix_epoch(next_account)
		},
		None => true,
	};

	if keep_current_account {
		next.account = Some(current_account.clone());
		next.accounts = if current.accounts.is_empty() {
			vec![current_account]
		} else {
			current.accounts.clone()
		};
	} else if next.accounts.is_empty() && !current.accounts.is_empty() {
		next.accounts = current.accounts.clone();
	}
}

fn selected_marker_account(
	marker: &RunActivityMarkerRecord,
) -> Option<&CodexAccountActivitySummary> {
	marker
		.account
		.as_ref()
		.or_else(|| {
			marker.accounts.iter().find(|account| account.status.eq_ignore_ascii_case("selected"))
		})
		.or_else(|| marker.accounts.first())
}

fn account_marker_observed_unix_epoch(account: &CodexAccountActivitySummary) -> i64 {
	[account.selected_at_unix_epoch, account.checked_at_unix_epoch]
		.into_iter()
		.flatten()
		.max()
		.unwrap_or(0)
}

fn write_run_activity_marker_body_atomic(marker_path: &Path, body: &str) -> Result<()> {
	let parent = marker_path.parent().ok_or_else(|| {
		eyre::eyre!("activity marker path `{}` has no parent directory", marker_path.display())
	})?;
	let sequence =
		RUN_ACTIVITY_MARKER_WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
	let temp_path =
		parent.join(format!(".{RUN_ACTIVITY_MARKER_FILE}.{}.{}.tmp", process::id(), sequence,));
	let result = (|| -> Result<()> {
		let mut temp_file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;

		temp_file.write_all(body.as_bytes())?;
		temp_file.flush()?;

		drop(temp_file);

		fs::rename(&temp_path, marker_path)?;

		Ok(())
	})();

	if result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	result
}

fn serialize_run_activity_marker_record(marker: &RunActivityMarkerRecord) -> String {
	let mut body = String::new();

	if let Some(run_id) = &marker.run_id {
		body.push_str(&format!("run_id={run_id}\n"));
	}
	if let Some(attempt_number) = marker.attempt_number {
		body.push_str(&format!("attempt_number={attempt_number}\n"));
	}
	if let Some(process_id) = marker.process_id {
		body.push_str(&format!("process_id={process_id}\n"));
	}
	if let Some(host_boot_id) = &marker.host_boot_id {
		body.push_str(&format!("host_boot_id={host_boot_id}\n"));
	}
	if let Some(process_start_identity) = &marker.process_start_identity {
		body.push_str(&format!("process_start_identity={process_start_identity}\n"));
	}
	if let Some(last_activity_unix_epoch) = marker.last_activity_unix_epoch {
		body.push_str(&format!("last_activity_unix_epoch={last_activity_unix_epoch}\n"));
	}
	if let Some(last_protocol_activity_unix_epoch) = marker.last_protocol_activity_unix_epoch {
		body.push_str(&format!(
			"last_protocol_activity_unix_epoch={last_protocol_activity_unix_epoch}\n"
		));
	}
	if let Some(last_progress_unix_epoch) = marker.last_progress_unix_epoch {
		body.push_str(&format!("last_progress_unix_epoch={last_progress_unix_epoch}\n"));
	}
	if let Some(current_operation) = &marker.current_operation {
		body.push_str(&format!("current_operation={current_operation}\n"));
	}
	if let Some(thread_id) = &marker.thread_id {
		body.push_str(&format!("thread_id={thread_id}\n"));
	}
	if let Some(turn_id) = &marker.turn_id {
		body.push_str(&format!("turn_id={turn_id}\n"));
	}
	if let Some(thread_status) = &marker.thread_status {
		body.push_str(&format!("thread_status={thread_status}\n"));
	}

	if !marker.thread_active_flags.is_empty() {
		body.push_str(&format!("thread_active_flags={}\n", marker.thread_active_flags.join(",")));
	}

	if let Some(event_count) = marker.event_count {
		body.push_str(&format!("event_count={event_count}\n"));
	}
	if let Some(last_event_type) = &marker.last_event_type {
		body.push_str(&format!("last_event_type={last_event_type}\n"));
	}
	if let Some(effective_model) = &marker.effective_model {
		body.push_str(&format!("effective_model={effective_model}\n"));
	}
	if let Some(effective_model_provider) = &marker.effective_model_provider {
		body.push_str(&format!("effective_model_provider={effective_model_provider}\n"));
	}
	if let Some(effective_cwd) = &marker.effective_cwd {
		body.push_str(&format!("effective_cwd={effective_cwd}\n"));
	}
	if let Some(effective_approval_policy) = &marker.effective_approval_policy {
		body.push_str(&format!("effective_approval_policy={effective_approval_policy}\n"));
	}
	if let Some(effective_approvals_reviewer) = &marker.effective_approvals_reviewer {
		body.push_str(&format!("effective_approvals_reviewer={effective_approvals_reviewer}\n"));
	}
	if let Some(effective_sandbox_mode) = &marker.effective_sandbox_mode {
		body.push_str(&format!("effective_sandbox_mode={effective_sandbox_mode}\n"));
	}
	if let Some(child_agent_activity) = &marker.child_agent_activity
		&& let Ok(summary_json) = serde_json::to_string(child_agent_activity)
	{
		body.push_str(&format!("child_agent_activity={summary_json}\n"));
	}
	if let Some(protocol_activity) = &marker.protocol_activity
		&& let Ok(summary_json) = serde_json::to_string(protocol_activity)
	{
		body.push_str(&format!("protocol_activity={summary_json}\n"));
	}

	append_run_activity_marker_account_fields(&mut body, marker);
	append_run_activity_marker_retry_fields(&mut body, marker);

	body
}

fn append_run_activity_marker_account_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
	if let Some(account) = &marker.account
		&& let Ok(summary_json) = serde_json::to_string(account)
	{
		body.push_str(&format!("account={summary_json}\n"));
	}

	if !marker.accounts.is_empty()
		&& let Ok(accounts_json) = serde_json::to_string(&marker.accounts)
	{
		body.push_str(&format!("accounts={accounts_json}\n"));
	}
}

fn append_run_activity_marker_retry_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
	if let Some(retry_budget_attempt_count) = marker.retry_budget_attempt_count {
		body.push_str(&format!("retry_budget_attempt_count={retry_budget_attempt_count}\n"));
	}
	if let Some(retry_kind) = &marker.retry_kind {
		body.push_str(&format!("retry_kind={retry_kind}\n"));
	}
	if let Some(retry_ready_at_unix_epoch) = marker.retry_ready_at_unix_epoch {
		body.push_str(&format!("retry_ready_at_unix_epoch={retry_ready_at_unix_epoch}\n"));
	}
}

fn parse_marker_list(value: &str) -> Vec<String> {
	value.split(',').filter(|part| !part.is_empty()).map(str::to_owned).collect()
}
