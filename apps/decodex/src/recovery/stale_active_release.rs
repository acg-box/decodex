//! Apply path for stale-active release recovery.

mod claims;
mod diagnostic;
mod state_restore;
mod worktree_cleanup;

use crate::{
	config::ServiceConfig,
	prelude::Result,
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT,
		context::RecoveryContext,
		reports::StaleActiveDiagnostic,
		stale_active_authority,
		stale_active_diagnosis::{self},
	},
	state::{RUN_CONTROL_CHANNEL_STATUS_FAILED, StateStore},
	tracker::{self, IssueTracker},
	workflow::WorkflowDocument,
};

pub(super) fn preflight_stale_active_worktree_cleanup(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	worktree_cleanup::preflight_stale_active_worktree_cleanup(state_store, diagnostic)
}

pub(super) fn apply_stale_active_release(
	context: &RecoveryContext,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	apply_stale_active_release_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostic,
	)
}

pub(super) fn apply_stale_active_release_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let diagnostic = diagnostic::refreshed_stale_active_release_diagnostic(
		tracker,
		config,
		workflow,
		state_store,
		diagnostic,
	)?;
	let worktree_cleanup =
		worktree_cleanup::preflight_stale_active_worktree_cleanup_plan(state_store, &diagnostic)?;

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&diagnostic,
	)?;

	let cleared_run_lease =
		claims::clear_stale_active_dead_run_claims_before_release(state_store, &diagnostic)?;

	claims::ensure_stale_active_run_claim_guard(config, state_store, &diagnostic)?;

	let active_label = tracker::automation_active_label(config.service_id());

	if let Some(run_id) = diagnostic.latest_run_id.as_deref()
		&& let Some(attempt_number) = diagnostic.latest_attempt_number
	{
		if diagnostic
			.latest_attempt_status
			.as_deref()
			.is_some_and(claims::stale_active_attempt_status_needs_terminal_guard)
		{
			state_store.update_run_status(run_id, GHOST_LANE_TERMINAL_STATUS)?;
		}

		state_store.retire_run_control_channel_for_attempt(
			run_id,
			attempt_number,
			RUN_CONTROL_CHANNEL_STATUS_FAILED,
		)?;
	}

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&diagnostic,
	)?;
	worktree_cleanup::cleanup_stale_active_worktree_mapping(
		config,
		workflow,
		state_store,
		&diagnostic,
		worktree_cleanup,
	)?;

	if let Some(run_id) = diagnostic.latest_run_id.as_deref()
		&& let Some(attempt_number) = diagnostic.latest_attempt_number
	{
		state_store
			.append_private_execution_event(
				&diagnostic.project_id,
				&diagnostic.issue_id,
				run_id,
				attempt_number,
				STALE_ACTIVE_RELEASE_EVENT,
				serde_json::json!({
					"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
					"event": STALE_ACTIVE_RELEASE_EVENT,
					"phase": "local_cleanup_complete_before_active_label_release",
					"classification": &diagnostic.classification,
					"reason": &diagnostic.reason,
					"issue_identifier": &diagnostic.issue_identifier,
					"terminal_status": GHOST_LANE_TERMINAL_STATUS,
					"active_label_release": "pending_final_mutation",
					"queue_label_preserved": diagnostic.queue_label_present,
					"cleared_run_lease": cleared_run_lease,
					"worktree_state": &diagnostic.worktree_state,
					"evidence": &diagnostic.evidence,
					"blockers": &diagnostic.blockers,
					"next_action": "ordinary automation may continue after status readback confirms no current attention lane",
				}),
			)
			.map(|_| ())?;
	}

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&diagnostic,
	)?;
	claims::ensure_stale_active_run_claim_guard(config, state_store, &diagnostic)?;

	let final_diagnostic = diagnostic::refreshed_stale_active_release_diagnostic(
		tracker,
		config,
		workflow,
		state_store,
		&diagnostic,
	)?;

	stale_active_authority::ensure_stale_active_review_authority_missing(
		tracker,
		state_store,
		&final_diagnostic,
	)?;
	claims::ensure_stale_active_run_claim_guard(config, state_store, &final_diagnostic)?;

	let issue =
		stale_active_diagnosis::lookup_stale_active_issue(tracker, &diagnostic.issue_identifier)?;

	state_restore::restore_stale_active_startable_state_if_queued(
		tracker,
		workflow,
		&issue,
		&final_diagnostic,
	)?;
	tracker::set_issue_label_presence(tracker, &issue, &active_label, false)?;

	Ok(())
}

#[cfg(test)]
pub(crate) fn clear_stale_active_dead_run_claims_before_release(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<bool> {
	claims::clear_stale_active_dead_run_claims_before_release(state_store, diagnostic)
}

#[cfg(test)]
pub(crate) fn ensure_stale_active_run_claim_guard(
	config: &ServiceConfig,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	claims::ensure_stale_active_run_claim_guard(config, state_store, diagnostic)
}
