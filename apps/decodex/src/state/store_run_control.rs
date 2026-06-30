//! Run-control channel publishing, action resolution, and audit persistence.

use super::{
	PrivateExecutionEvent, ProjectRunStatus, RUN_CONTROL_ACTION_ACCEPTED,
	RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_ACTION_FALLBACK,
	RUN_CONTROL_ACTION_REJECTED, RUN_CONTROL_ACTION_TIMED_OUT, RUN_CONTROL_CHANNEL_STATUS_ACTIVE,
	RUN_CONTROL_CHANNEL_STATUS_COMPLETED, RUN_CONTROL_CHANNEL_STATUS_FAILED, Result,
	RunControlActionOutcomeRequest, RunControlActionReceipt, RunControlActionRequest,
	RunControlChannel, RunControlChannelRecord, StateData, StateStore, Value, eyre,
	running_run_attempt_status, timestamp_parts,
};
use std::path::{Path, PathBuf};

#[cfg_attr(not(test), allow(dead_code))]
struct RunControlActionResolution {
	audit_target: RunControlAuditTarget,
	outcome: String,
	reason: String,
	channel: Option<RunControlChannel>,
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct RunControlAuditTarget {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	attempt_status: Option<String>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	source: String,
	action: String,
	timeout_ms: Option<i64>,
	current_thread_id: Option<String>,
	current_turn_id: Option<String>,
	metadata: Option<Value>,
	context: Option<Value>,
	branch_name: Option<String>,
	worktree_path: Option<PathBuf>,
	run_lease: Option<bool>,
	event_count: Option<i64>,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	channel: Option<RunControlChannel>,
}

impl StateStore {
	/// Publish the local control channel for an active attempt when the runtime owns it.
	pub(crate) fn publish_run_control_channel_for_active_attempt(
		&self,
		run_id: &str,
		attempt_number: i64,
		channel_path: &Path,
		transport: &str,
	) -> Result<Option<RunControlChannel>> {
		validate_run_control_channel_inputs(run_id, attempt_number, channel_path, transport)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let Some(attempt) = state.run_attempts.get(run_id).cloned() else {
			return Ok(None);
		};

		if attempt.attempt_number != attempt_number {
			return Ok(None);
		}

		let Some(lease) = state.leases.get(&attempt.issue_id) else {
			return Ok(None);
		};

		if lease.run_id != run_id {
			return Ok(None);
		}

		let (published_at, published_at_unix) = state
			.control_channels
			.get(run_id)
			.filter(|channel| channel.attempt_number == attempt_number)
			.map_or_else(
				|| (now.text.clone(), now.unix),
				|channel| (channel.published_at.clone(), channel.published_at_unix),
			);
		let channel = RunControlChannelRecord {
			project_id: lease.project_id.clone(),
			issue_id: attempt.issue_id.clone(),
			run_id: run_id.to_owned(),
			attempt_number,
			transport: transport.to_owned(),
			channel_path: channel_path.to_path_buf(),
			status: RUN_CONTROL_CHANNEL_STATUS_ACTIVE.to_owned(),
			published_at,
			published_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.control_channels.insert(run_id.to_owned(), channel.clone());
		self.upsert_run_control_channel_locked(&channel)?;

		Ok(Some(channel.as_public()))
	}

	/// Mark a run-control channel as no longer active for an attempt.
	pub(crate) fn retire_run_control_channel_for_attempt(
		&self,
		run_id: &str,
		attempt_number: i64,
		status: &str,
	) -> Result<()> {
		validate_run_control_channel_status(status)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let Some(channel) = state.control_channels.get_mut(run_id) else {
			return Ok(());
		};

		if channel.attempt_number != attempt_number {
			return Ok(());
		}

		channel.status = status.to_owned();
		channel.updated_at = now.text;
		channel.updated_at_unix = now.unix;

		let channel = channel.clone();

		self.upsert_run_control_channel_locked(&channel)
	}

	/// Resolve a local run-control request against run lease ownership and audit it.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn resolve_run_control_action(
		&self,
		request: RunControlActionRequest<'_>,
	) -> Result<RunControlActionReceipt> {
		validate_run_control_action_request(&request)?;

		let resolution = {
			let mut state = self.lock_without_refresh()?;

			self.refresh_project_run_metadata_state_locked(&mut state, request.project_id)?;
			self.refresh_run_attempt_identities_from_worktree_markers_locked(
				&mut state,
				request.project_id,
			)?;

			resolve_run_control_action_locked(&state, &request)
		};
		let event = self.append_run_control_audit_event(
			&resolution.audit_target,
			&resolution.outcome,
			&resolution.reason,
			None,
		)?;
		let receipt_channel =
			resolution.channel.clone().or_else(|| resolution.audit_target.channel.clone());

		Ok(RunControlActionReceipt {
			project_id: resolution.audit_target.project_id,
			issue_id: resolution.audit_target.issue_id,
			run_id: resolution.audit_target.run_id,
			attempt_number: resolution.audit_target.attempt_number,
			thread_id: resolution.audit_target.thread_id,
			turn_id: resolution.audit_target.turn_id,
			current_thread_id: resolution.audit_target.current_thread_id,
			current_turn_id: resolution.audit_target.current_turn_id,
			source: resolution.audit_target.source,
			action: resolution.audit_target.action,
			outcome: resolution.outcome,
			reason: resolution.reason,
			audit_record_id: event.record_id(),
			metadata: resolution.audit_target.metadata,
			context: resolution.audit_target.context,
			channel: receipt_channel,
		})
	}

	/// Append a follow-up audit outcome for an already resolved control request.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn record_run_control_action_outcome(
		&self,
		receipt: &RunControlActionReceipt,
		outcome: &str,
		reason: &str,
	) -> Result<PrivateExecutionEvent> {
		validate_run_control_action_outcome(outcome)?;
		validate_required_run_control_field("reason", reason)?;

		let target = RunControlAuditTarget {
			project_id: receipt.project_id.clone(),
			issue_id: receipt.issue_id.clone(),
			run_id: receipt.run_id.clone(),
			attempt_number: receipt.attempt_number,
			thread_id: receipt.thread_id.clone(),
			turn_id: receipt.turn_id.clone(),
			current_thread_id: receipt.current_thread_id.clone(),
			current_turn_id: receipt.current_turn_id.clone(),
			source: receipt.source.clone(),
			action: receipt.action.clone(),
			timeout_ms: None,
			metadata: receipt.metadata.clone(),
			context: receipt.context.clone(),
			attempt_status: None,
			branch_name: None,
			worktree_path: None,
			run_lease: None,
			event_count: None,
			last_event_type: None,
			last_event_at: None,
			channel: receipt.channel.clone(),
		};

		self.append_run_control_audit_event(&target, outcome, reason, Some(receipt.audit_record_id))
	}

	/// Append a follow-up audit outcome for a control action handled from a channel request.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn record_run_control_action_delivery_outcome(
		&self,
		request: RunControlActionOutcomeRequest<'_>,
	) -> Result<PrivateExecutionEvent> {
		validate_run_control_action_outcome(request.outcome)?;
		validate_required_run_control_field("reason", request.reason)?;

		let target = RunControlAuditTarget {
			project_id: request.project_id.to_owned(),
			issue_id: request.issue_id.to_owned(),
			run_id: request.run_id.to_owned(),
			attempt_number: request.attempt_number,
			thread_id: request.thread_id.map(str::to_owned),
			turn_id: request.turn_id.map(str::to_owned),
			current_thread_id: request.current_thread_id.map(str::to_owned),
			current_turn_id: request.current_turn_id.map(str::to_owned),
			source: request.source.to_owned(),
			action: request.action.to_owned(),
			timeout_ms: request.timeout_ms,
			metadata: request.metadata.cloned(),
			context: None,
			attempt_status: None,
			branch_name: None,
			worktree_path: None,
			run_lease: None,
			event_count: None,
			last_event_type: None,
			last_event_at: None,
			channel: request.channel.cloned(),
		};

		self.append_run_control_audit_event(
			&target,
			request.outcome,
			request.reason,
			request.parent_record_id,
		)
	}

	#[cfg_attr(not(test), allow(dead_code))]
	fn append_run_control_audit_event(
		&self,
		target: &RunControlAuditTarget,
		outcome: &str,
		reason: &str,
		parent_record_id: Option<i64>,
	) -> Result<PrivateExecutionEvent> {
		validate_run_control_action_outcome(outcome)?;

		let channel = target.channel.as_ref();
		let failure_class = run_control_action_failure_class(&target.action, outcome, reason);
		let payload = serde_json::json!({
			"schema": "decodex.run_control_action/v1",
			"action": target.action,
			"source": target.source,
			"outcome": outcome,
			"reason": reason,
			"failure_class": failure_class,
			"parent_record_id": parent_record_id,
			"requested": {
				"project_id": target.project_id,
				"issue_id": target.issue_id,
				"run_id": target.run_id,
				"attempt_number": target.attempt_number,
				"thread_id": target.thread_id,
				"turn_id": target.turn_id,
				"timeout_ms": target.timeout_ms,
			},
			"observed": {
				"thread_id": target.current_thread_id.as_deref(),
				"turn_id": target.current_turn_id.as_deref(),
			},
			"lane": {
				"attempt_status": target.attempt_status.as_deref(),
				"run_lease": target.run_lease,
				"branch": target.branch_name.as_deref(),
				"worktree_path": target.worktree_path.as_ref().map(|path| path.display().to_string()),
				"event_count": target.event_count,
				"last_event_type": target.last_event_type.as_deref(),
				"last_event_at": target.last_event_at.as_deref(),
			},
			"metadata": target.metadata.as_ref(),
			"context": target.context.as_ref(),
			"channel": channel.map(|channel| serde_json::json!({
				"transport": channel.transport(),
				"channel_path": channel.channel_path().display().to_string(),
				"status": channel.status(),
				"published_at": channel.published_at(),
				"updated_at": channel.updated_at(),
				"path_exists": channel.channel_path().exists(),
			})),
		});

		self.append_private_execution_event(
			&target.project_id,
			&target.issue_id,
			&target.run_id,
			target.attempt_number,
			"control_action",
			payload,
		)
	}
}

#[cfg_attr(not(test), allow(dead_code))]
fn resolve_run_control_action_locked(
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
	if !running_run_attempt_status(&attempt.status) {
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

fn validate_run_control_channel_inputs(
	run_id: &str,
	attempt_number: i64,
	channel_path: &Path,
	transport: &str,
) -> Result<()> {
	validate_required_run_control_field("run_id", run_id)?;
	validate_required_run_control_field("transport", transport)?;

	if attempt_number < 1 {
		eyre::bail!("run-control attempt_number must be positive");
	}
	if channel_path.as_os_str().is_empty() {
		eyre::bail!("run-control channel_path must not be empty");
	}

	Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_run_control_action_request(request: &RunControlActionRequest<'_>) -> Result<()> {
	validate_required_run_control_field("project_id", request.project_id)?;
	validate_required_run_control_field("issue_id", request.issue_id)?;
	validate_required_run_control_field("run_id", request.run_id)?;
	validate_required_run_control_field("source", request.source)?;
	validate_required_run_control_field("action", request.action)?;

	if request.attempt_number < 1 {
		eyre::bail!("run-control attempt_number must be positive");
	}

	if let Some(timeout_ms) = request.timeout_ms
		&& timeout_ms < 0
	{
		eyre::bail!("run-control timeout_ms must not be negative");
	}

	Ok(())
}

fn validate_required_run_control_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("run-control {name} must not be empty");
	}

	Ok(())
}

fn validate_run_control_channel_status(status: &str) -> Result<()> {
	if !matches!(
		status,
		RUN_CONTROL_CHANNEL_STATUS_ACTIVE
			| RUN_CONTROL_CHANNEL_STATUS_COMPLETED
			| RUN_CONTROL_CHANNEL_STATUS_FAILED
	) {
		eyre::bail!("unsupported run-control channel status `{status}`");
	}

	Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_run_control_action_outcome(outcome: &str) -> Result<()> {
	if !matches!(
		outcome,
		RUN_CONTROL_ACTION_ACCEPTED
			| RUN_CONTROL_ACTION_REJECTED
			| RUN_CONTROL_ACTION_COMPLETED
			| RUN_CONTROL_ACTION_FAILED
			| RUN_CONTROL_ACTION_TIMED_OUT
			| RUN_CONTROL_ACTION_FALLBACK
	) {
		eyre::bail!("unsupported run-control action outcome `{outcome}`");
	}

	Ok(())
}

fn run_control_action_failure_class(
	action: &str,
	outcome: &str,
	reason: &str,
) -> Option<&'static str> {
	if !matches!(
		outcome,
		RUN_CONTROL_ACTION_REJECTED
			| RUN_CONTROL_ACTION_FAILED
			| RUN_CONTROL_ACTION_TIMED_OUT
			| RUN_CONTROL_ACTION_FALLBACK
	) {
		return None;
	}
	if action == "steer" && reason == "turn_mismatch" {
		return Some("stale_expected_turn_id");
	}
	if action == "steer" && reason == "active_turn_not_steerable" {
		return Some("active_turn_not_steerable");
	}
	if action == "steer" && reason == "app_server_turn_steer_unsupported" {
		return Some("app_server_turn_steer_unsupported");
	}

	Some("run_control_action_failed")
}
