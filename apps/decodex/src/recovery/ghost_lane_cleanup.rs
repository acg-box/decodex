//! Apply and live-status gates for ghost-lane recovery.

use crate::{
	config::ServiceConfig,
	orchestrator,
	prelude::{Result, eyre},
	state::{RUN_CONTROL_CHANNEL_STATUS_FAILED, StateStore},
	tracker::IssueTracker,
	workflow::WorkflowDocument,
};

use super::{
	GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT, GHOST_LANE_TERMINAL_STATUS,
	context::RecoveryContext,
	reports::{GhostLaneDiagnostic, render_ghost_lane_issue},
};

pub(super) fn apply_ghost_lane_cleanup(
	state_store: &StateStore,
	diagnostic: &GhostLaneDiagnostic,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			&diagnostic.project_id,
			&diagnostic.issue_id,
			&diagnostic.run_id,
			diagnostic.attempt_number,
			GHOST_LANE_CLEANUP_EVENT,
			serde_json::json!({
				"schema": "decodex.ghost_lane_recovery_private_event/1",
				"event": GHOST_LANE_CLEANUP_EVENT,
				"classification": &diagnostic.classification,
				"reason": &diagnostic.reason,
				"issue_identifier": &diagnostic.issue_identifier,
				"terminal_status": GHOST_LANE_TERMINAL_STATUS,
				"cleared_run_lease": true,
				"evidence": &diagnostic.evidence,
				"blockers": &diagnostic.blockers,
				"next_action": "ordinary automation may continue after status readback confirms no current attention lane",
			}),
		)
		.map(|_| ())?;
	state_store.update_run_status(&diagnostic.run_id, GHOST_LANE_TERMINAL_STATUS)?;
	state_store.retire_run_control_channel_for_attempt(
		&diagnostic.run_id,
		diagnostic.attempt_number,
		RUN_CONTROL_CHANNEL_STATUS_FAILED,
	)?;

	if let Some(mapping) = state_store.worktree_for_issue(&diagnostic.issue_id)?
		&& !mapping.worktree_path().exists()
	{
		state_store.clear_worktree(&diagnostic.issue_id)?;
	}

	state_store.clear_lease(&diagnostic.issue_id)
}

pub(super) fn ensure_ghost_lane_live_status_allows_cleanup(
	context: &RecoveryContext,
	diagnostic: &GhostLaneDiagnostic,
) -> Result<()> {
	ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostic,
	)
}

pub(super) fn ensure_ghost_lane_live_status_allows_cleanup_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &GhostLaneDiagnostic,
) -> Result<()>
where
	T: IssueTracker,
{
	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		tracker,
		config,
		workflow,
		state_store,
		&diagnostic.issue_id,
		&diagnostic.run_id,
	)?;

	if blockers.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"`recover ghost-lane cleanup` refused `{}` because live status reported blockers: {}",
		render_ghost_lane_issue(diagnostic),
		blockers.join(", ")
	)
}

pub(super) fn apply_ghost_lane_live_status_blockers(
	context: &RecoveryContext,
	diagnostics: &mut [GhostLaneDiagnostic],
) -> Result<()> {
	apply_ghost_lane_live_status_blockers_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostics,
	)
}

pub(super) fn apply_ghost_lane_live_status_blockers_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostics: &mut [GhostLaneDiagnostic],
) -> Result<()>
where
	T: IssueTracker,
{
	for diagnostic in diagnostics {
		let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
			tracker,
			config,
			workflow,
			state_store,
			&diagnostic.issue_id,
			&diagnostic.run_id,
		)?;

		if blockers.is_empty() {
			continue;
		}

		diagnostic.classification = String::from(GHOST_LANE_BLOCKED_CLASSIFICATION);
		diagnostic.reason = String::from("status_safety_check_blocked");
		diagnostic.next_action = String::from(
			"Preserve attention and inspect the listed blockers before using a recovery command.",
		);
		diagnostic.blockers = super::sorted_unique(
			diagnostic
				.blockers
				.iter()
				.cloned()
				.chain(blockers.into_iter().map(|blocker| format!("status:{blocker}")))
				.collect(),
		);
	}

	Ok(())
}
