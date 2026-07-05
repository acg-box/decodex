mod event;

use crate::{
	prelude::Result,
	state::{
		PrivateExecutionEvent, RunControlActionOutcomeRequest, RunControlActionReceipt,
		RunControlActionRequest, StateStore,
		store_run_control::{resolution, types::RunControlAuditTarget, validation},
	},
};

impl StateStore {
	/// Resolve a local run-control request against run lease ownership and audit it.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn resolve_run_control_action(
		&self,
		request: RunControlActionRequest<'_>,
	) -> Result<RunControlActionReceipt> {
		validation::validate_run_control_action_request(&request)?;

		let resolution = {
			let mut state = self.lock_without_refresh()?;

			self.refresh_project_run_metadata_state_locked(&mut state, request.project_id)?;
			self.refresh_run_attempt_identities_from_worktree_markers_locked(
				&mut state,
				request.project_id,
			)?;

			resolution::resolve_run_control_action_locked(&state, &request)
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
		validation::validate_run_control_action_outcome(outcome)?;
		validation::validate_required_run_control_field("reason", reason)?;

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
		validation::validate_run_control_action_outcome(request.outcome)?;
		validation::validate_required_run_control_field("reason", request.reason)?;

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
		event::append_run_control_audit_event(self, target, outcome, reason, parent_record_id)
	}
}
