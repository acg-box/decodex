use crate::state::run_activity_marker::record::RunActivityMarkerRecord;

pub(crate) fn serialize_run_activity_marker_record(marker: &RunActivityMarkerRecord) -> String {
	let mut body = String::new();

	append_run_activity_marker_identity_fields(&mut body, marker);
	append_run_activity_marker_thread_fields(&mut body, marker);
	append_run_activity_marker_runtime_fields(&mut body, marker);
	append_run_activity_marker_summary_fields(&mut body, marker);
	append_run_activity_marker_account_fields(&mut body, marker);
	append_run_activity_marker_retry_fields(&mut body, marker);

	body
}

fn append_run_activity_marker_identity_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
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
}

fn append_run_activity_marker_thread_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
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
}

fn append_run_activity_marker_runtime_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
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
}

fn append_run_activity_marker_summary_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
	if let Some(event_count) = marker.event_count {
		body.push_str(&format!("event_count={event_count}\n"));
	}
	if let Some(last_event_type) = &marker.last_event_type {
		body.push_str(&format!("last_event_type={last_event_type}\n"));
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
