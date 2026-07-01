use std::{
	fs::{self, OpenOptions},
	io::{ErrorKind, Write},
	path::Path,
	process,
	sync::atomic::{AtomicU64, Ordering},
};

use crate::{
	prelude::{Result, eyre},
	state::{CodexAccountActivitySummary, RUN_ACTIVITY_MARKER_FILE},
};

use super::RunActivityMarkerRecord;

static RUN_ACTIVITY_MARKER_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
			"last_activity_unix_epoch" =>
				marker.last_activity_unix_epoch = value.parse::<i64>().ok(),
			"last_protocol_activity_unix_epoch" =>
				marker.last_protocol_activity_unix_epoch = value.parse::<i64>().ok(),
			"last_progress_unix_epoch" =>
				marker.last_progress_unix_epoch = value.parse::<i64>().ok(),
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
			"effective_approval_policy" =>
				marker.effective_approval_policy = Some(value.to_owned()),
			"effective_approvals_reviewer" =>
				marker.effective_approvals_reviewer = Some(value.to_owned()),
			"effective_sandbox_mode" => marker.effective_sandbox_mode = Some(value.to_owned()),
			"child_agent_activity" =>
				marker.child_agent_activity = serde_json::from_str(value).ok(),
			"protocol_activity" => marker.protocol_activity = serde_json::from_str(value).ok(),
			"account" => marker.account = serde_json::from_str(value).ok(),
			"accounts" =>
				if let Ok(accounts) = serde_json::from_str(value) {
					marker.accounts = accounts;
				},
			"retry_budget_attempt_count" =>
				marker.retry_budget_attempt_count = value.parse::<i64>().ok(),
			"retry_kind" => marker.retry_kind = Some(value.to_owned()),
			"retry_ready_at_unix_epoch" =>
				marker.retry_ready_at_unix_epoch = value.parse::<i64>().ok(),
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
		Some(next_account) =>
			account_marker_observed_unix_epoch(&current_account)
				> account_marker_observed_unix_epoch(next_account),
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
	let sequence = RUN_ACTIVITY_MARKER_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
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
