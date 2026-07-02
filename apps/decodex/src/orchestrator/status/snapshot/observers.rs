use std::time::Instant;

use time::OffsetDateTime;

use crate::orchestrator::status::{
	self, IssueTracker, LiveOperatorStatusObserverContext, OperatorConnectorBackoffStatus,
	OperatorStatusSnapshot, ServiceConfig, StateStore, TrackerConnectorBackoff,
	TrackerObserverOutcome,
};
use crate::prelude::Result;

pub(in crate::orchestrator) fn hydrate_live_operator_external_observers<T>(
	context: LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<()>
where
	T: IssueTracker,
{
	let stale_terminal_local_issue_ids =
		status::stale_terminal_local_issue_ids(context.project, context.state_store)?;
	let mut paused = pause_operator_snapshot_for_stored_tracker_backoff(&context, snapshot)?;

	if !paused {
		paused = apply_tracker_observer_outcome(
			status::hydrate_operator_run_rows_from_tracker(
				context.tracker,
				context.project,
				context.workflow,
				snapshot,
				context.run_issue_metadata_hydration,
				&stale_terminal_local_issue_ids,
			),
			snapshot,
			context.state_store,
			context.project,
			"run_issue_metadata_unavailable",
		);
	}
	if !paused && context.hydrate_history_ledger {
		paused = apply_tracker_observer_outcome(
			status::hydrate_history_lanes_from_linear_ledger(
				context.tracker,
				context.project,
				snapshot,
				&stale_terminal_local_issue_ids,
			),
			snapshot,
			context.state_store,
			context.project,
			"execution_ledger_status_unavailable",
		);
	}
	if !paused {
		paused = hydrate_queued_candidate_status_observer(&context, snapshot);
	}
	if !paused {
		paused = hydrate_post_review_lane_status_observer(&context, snapshot)?;
	}
	if paused {
		if snapshot.post_review_lanes.is_empty() {
			snapshot.post_review_lanes = status::build_degraded_post_review_lane_statuses(
				context.project,
				context.state_store,
				context.review_state_inspector,
			)?;
		}

		add_operator_snapshot_warning(snapshot, "external_observer_status_skipped");
	}

	Ok(())
}

pub(in crate::orchestrator) fn pause_operator_snapshot_for_stored_tracker_backoff<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<bool> {
	let Some(backoff) = status::active_stored_tracker_backoff_status(
		context.state_store,
		context.project.service_id(),
	)?
	else {
		return Ok(false);
	};

	add_tracker_backoff_to_operator_snapshot(snapshot, &backoff);

	Ok(true)
}

pub(in crate::orchestrator) fn apply_tracker_observer_outcome(
	outcome: TrackerObserverOutcome,
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	unavailable_warning: &'static str,
) -> bool {
	match outcome {
		TrackerObserverOutcome::Ok => false,
		TrackerObserverOutcome::Unavailable => {
			add_operator_snapshot_warning(snapshot, unavailable_warning);

			false
		},
		TrackerObserverOutcome::Backoff(backoff) => {
			pause_operator_snapshot_for_tracker_backoff(snapshot, state_store, project, &backoff);

			true
		},
	}
}

pub(in crate::orchestrator) fn pause_operator_snapshot_for_tracker_backoff(
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	backoff: &TrackerConnectorBackoff,
) {
	status::persist_tracker_backoff_state(state_store, project.service_id(), backoff);

	let backoff = backoff
		.to_operator_status(project.service_id(), OffsetDateTime::now_utc().unix_timestamp());

	add_tracker_backoff_to_operator_snapshot(snapshot, &backoff);
}

pub(in crate::orchestrator) fn hydrate_queued_candidate_status_observer<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> bool
where
	T: IssueTracker,
{
	match status::build_queued_candidate_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
	) {
		Ok(queued_candidates) => {
			snapshot.queued_candidates = queued_candidates;

			false
		},
		Err(error) => {
			let Some(backoff) = status::tracker_connector_backoff(
				&error,
				Instant::now(),
				"queued_candidate_status",
			) else {
				let _ = error;

				tracing::warn!(
					"Skipped queued candidate status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				add_operator_snapshot_warning(snapshot, "queued_candidate_status_unavailable");

				return false;
			};

			pause_operator_snapshot_for_tracker_backoff(
				snapshot,
				context.state_store,
				context.project,
				&backoff,
			);

			true
		},
	}
}

pub(in crate::orchestrator) fn hydrate_post_review_lane_status_observer<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<bool>
where
	T: IssueTracker,
{
	match status::build_post_review_lane_statuses_and_hydrate_worktrees(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		context.review_state_inspector,
		snapshot,
	) {
		Ok(post_review_lanes) => {
			snapshot.post_review_lanes = post_review_lanes;

			Ok(false)
		},
		Err(error) => {
			let Some(backoff) = status::tracker_connector_backoff(
				&error,
				Instant::now(),
				"post_review_lane_status",
			) else {
				let _ = error;

				tracing::warn!(
					"Skipped post-review lane status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				add_operator_snapshot_warning(snapshot, "post_review_lane_status_unavailable");

				return Ok(false);
			};

			pause_operator_snapshot_for_tracker_backoff(
				snapshot,
				context.state_store,
				context.project,
				&backoff,
			);

			snapshot.post_review_lanes = status::build_degraded_post_review_lane_statuses(
				context.project,
				context.state_store,
				context.review_state_inspector,
			)?;

			Ok(true)
		},
	}
}

pub(in crate::orchestrator) fn add_tracker_backoff_to_operator_snapshot(
	snapshot: &mut OperatorStatusSnapshot,
	backoff: &OperatorConnectorBackoffStatus,
) {
	add_operator_snapshot_warning(snapshot, &backoff.warning);

	if !snapshot.connector_backoffs.iter().any(|existing| {
		existing.project_id == backoff.project_id && existing.connector == backoff.connector
	}) {
		snapshot.connector_backoffs.push(backoff.clone());
	}
}

pub(in crate::orchestrator) fn add_operator_snapshot_warning(
	snapshot: &mut OperatorStatusSnapshot,
	warning: &str,
) {
	if !snapshot.warnings.iter().any(|existing| existing == warning) {
		snapshot.warnings.push(warning.to_owned());
	}
}
