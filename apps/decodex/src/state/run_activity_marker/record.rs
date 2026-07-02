use crate::state::{
	ChildAgentActivitySummary, CodexAccountActivitySummary, ProtocolActivitySummary,
};

#[derive(Clone, Default)]
pub(crate) struct RunActivityMarkerRecord {
	pub(in crate::state::run_activity_marker) run_id: Option<String>,
	pub(in crate::state::run_activity_marker) attempt_number: Option<i64>,
	pub(in crate::state::run_activity_marker) process_id: Option<u32>,
	pub(in crate::state::run_activity_marker) host_boot_id: Option<String>,
	pub(in crate::state::run_activity_marker) process_start_identity: Option<String>,
	pub(in crate::state::run_activity_marker) last_activity_unix_epoch: Option<i64>,
	pub(in crate::state::run_activity_marker) last_protocol_activity_unix_epoch: Option<i64>,
	pub(in crate::state::run_activity_marker) last_progress_unix_epoch: Option<i64>,
	pub(in crate::state::run_activity_marker) current_operation: Option<String>,
	pub(in crate::state::run_activity_marker) thread_id: Option<String>,
	pub(in crate::state::run_activity_marker) turn_id: Option<String>,
	pub(in crate::state::run_activity_marker) thread_status: Option<String>,
	pub(in crate::state::run_activity_marker) thread_active_flags: Vec<String>,
	pub(in crate::state::run_activity_marker) event_count: Option<i64>,
	pub(in crate::state::run_activity_marker) last_event_type: Option<String>,
	pub(in crate::state::run_activity_marker) effective_model: Option<String>,
	pub(in crate::state::run_activity_marker) effective_model_provider: Option<String>,
	pub(in crate::state::run_activity_marker) effective_cwd: Option<String>,
	pub(in crate::state::run_activity_marker) effective_approval_policy: Option<String>,
	pub(in crate::state::run_activity_marker) effective_approvals_reviewer: Option<String>,
	pub(in crate::state::run_activity_marker) effective_sandbox_mode: Option<String>,
	pub(in crate::state::run_activity_marker) child_agent_activity:
		Option<ChildAgentActivitySummary>,
	pub(in crate::state::run_activity_marker) protocol_activity: Option<ProtocolActivitySummary>,
	pub(in crate::state::run_activity_marker) account: Option<CodexAccountActivitySummary>,
	pub(in crate::state::run_activity_marker) accounts: Vec<CodexAccountActivitySummary>,
	pub(in crate::state::run_activity_marker) retry_budget_attempt_count: Option<i64>,
	pub(in crate::state::run_activity_marker) retry_kind: Option<String>,
	pub(in crate::state::run_activity_marker) retry_ready_at_unix_epoch: Option<i64>,
}

pub(in crate::state::run_activity_marker) fn run_activity_marker_record_for_attempt(
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
