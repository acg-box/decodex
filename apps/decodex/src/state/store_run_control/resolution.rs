use std::path::Path;

use crate::state::{
	self, ProjectRunStatus, RUN_CONTROL_ACTION_ACCEPTED, RUN_CONTROL_ACTION_REJECTED,
	RUN_CONTROL_CHANNEL_STATUS_ACTIVE, RunControlActionRequest, RunControlChannelRecord, StateData,
	store_run_control::types::{RunControlActionResolution, RunControlAuditTarget},
};

#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::state::store_run_control) fn resolve_run_control_action_locked(
	state: &StateData,
	request: &RunControlActionRequest<'_>,
) -> RunControlActionResolution {
	let Some(attempt) = state.run_attempts.get(request.run_id) else {
		return rejected_run_control_resolution(request, None, "run_not_found");
	};
	let audit_project_id = state
		.control_channels
		.get(request.run_id)
		.map(|channel| channel.project_id.clone())
		.or_else(|| state.project_id_for_run(&attempt.issue_id, &attempt.run_id))
		.unwrap_or_else(|| request.project_id.to_owned());
	let project_run_status = state.project_run_status(&audit_project_id, attempt);
	let control_channel =
		project_run_status.as_ref().and_then(|status| status.control_channel().cloned()).or_else(
			|| state.control_channels.get(request.run_id).map(RunControlChannelRecord::as_public),
		);
	let audit_target = RunControlAuditTarget {
		project_id: audit_project_id,
		issue_id: attempt.issue_id.clone(),
		run_id: attempt.run_id.clone(),
		attempt_number: attempt.attempt_number,
		attempt_status: Some(attempt.status.clone()),
		thread_id: request.thread_id.map(str::to_owned),
		turn_id: request.turn_id.map(str::to_owned),
		current_thread_id: attempt.thread_id.clone(),
		current_turn_id: attempt.turn_id.clone(),
		source: request.source.to_owned(),
		action: request.action.to_owned(),
		timeout_ms: request.timeout_ms,
		metadata: request.metadata.cloned(),
		context: request.context.cloned(),
		branch_name: project_run_status
			.as_ref()
			.and_then(|status| status.branch_name().map(str::to_owned)),
		worktree_path: project_run_status
			.as_ref()
			.and_then(|status| status.worktree_path().map(Path::to_path_buf)),
		run_lease: project_run_status.as_ref().map(ProjectRunStatus::run_lease),
		event_count: project_run_status.as_ref().map(ProjectRunStatus::event_count),
		last_event_type: project_run_status
			.as_ref()
			.and_then(|status| status.last_event_type().map(str::to_owned)),
		last_event_at: project_run_status
			.as_ref()
			.and_then(|status| status.last_event_at().map(str::to_owned)),
		channel: control_channel.clone(),
	};

	if attempt.issue_id != request.issue_id {
		return rejected_run_control_resolution(request, Some(audit_target), "issue_mismatch");
	}
	if attempt.attempt_number != request.attempt_number {
		return rejected_run_control_resolution(request, Some(audit_target), "attempt_mismatch");
	}
	if request.thread_id.is_some() && attempt.thread_id.as_deref() != request.thread_id {
		return rejected_run_control_resolution(request, Some(audit_target), "thread_mismatch");
	}
	if request.turn_id.is_some() && attempt.turn_id.as_deref() != request.turn_id {
		return rejected_run_control_resolution(request, Some(audit_target), "turn_mismatch");
	}

	let Some(lease) = state.leases.get(request.issue_id) else {
		return rejected_run_control_resolution(request, Some(audit_target), "run_lease_missing");
	};

	if lease.project_id != request.project_id {
		return rejected_run_control_resolution(request, Some(audit_target), "project_mismatch");
	}
	if lease.run_id != request.run_id {
		return rejected_run_control_resolution(request, Some(audit_target), "run_lease_mismatch");
	}
	if !state::running_run_attempt_status(&attempt.status) {
		return rejected_run_control_resolution(request, Some(audit_target), "run_not_active");
	}

	let Some(channel) = control_channel else {
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_missing",
		);
	};
	let audit_target = RunControlAuditTarget { channel: Some(channel.clone()), ..audit_target };

	if channel.project_id() != request.project_id
		|| channel.issue_id() != request.issue_id
		|| channel.attempt_number() != request.attempt_number
	{
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_identity_mismatch",
		);
	}
	if channel.status() != RUN_CONTROL_CHANNEL_STATUS_ACTIVE {
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_inactive",
		);
	}
	if !channel.channel_path().exists() {
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_missing",
		);
	}

	RunControlActionResolution {
		audit_target,
		outcome: RUN_CONTROL_ACTION_ACCEPTED.to_owned(),
		reason: String::from("run_lease_control_channel_resolved"),
		channel: Some(channel),
	}
}

#[cfg_attr(not(test), allow(dead_code))]
fn rejected_run_control_resolution(
	request: &RunControlActionRequest<'_>,
	audit_target: Option<RunControlAuditTarget>,
	reason: &str,
) -> RunControlActionResolution {
	RunControlActionResolution {
		audit_target: audit_target.unwrap_or_else(|| RunControlAuditTarget {
			project_id: request.project_id.to_owned(),
			issue_id: request.issue_id.to_owned(),
			run_id: request.run_id.to_owned(),
			attempt_number: request.attempt_number,
			attempt_status: None,
			thread_id: request.thread_id.map(str::to_owned),
			turn_id: request.turn_id.map(str::to_owned),
			current_thread_id: None,
			current_turn_id: None,
			source: request.source.to_owned(),
			action: request.action.to_owned(),
			timeout_ms: request.timeout_ms,
			metadata: request.metadata.cloned(),
			context: request.context.cloned(),
			branch_name: None,
			worktree_path: None,
			run_lease: None,
			event_count: None,
			last_event_type: None,
			last_event_at: None,
			channel: None,
		}),
		outcome: RUN_CONTROL_ACTION_REJECTED.to_owned(),
		reason: reason.to_owned(),
		channel: None,
	}
}
