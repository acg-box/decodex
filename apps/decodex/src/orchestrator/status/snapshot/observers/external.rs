use crate::{
	orchestrator::status::{
		self, IssueTracker, LiveOperatorStatusObserverContext, OperatorStatusSnapshot,
		snapshot::observers::{backoff, post_review, queued},
	},
	prelude::Result,
};

pub(crate) fn hydrate_live_operator_external_observers<T>(
	context: LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<()>
where
	T: IssueTracker,
{
	let stale_terminal_local_issue_ids =
		status::stale_terminal_local_issue_ids(context.project, context.state_store)?;
	let mut paused =
		backoff::pause_operator_snapshot_for_stored_tracker_backoff(&context, snapshot)?;

	if !paused {
		paused = backoff::apply_tracker_observer_outcome(
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
		paused = backoff::apply_tracker_observer_outcome(
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
		paused = queued::hydrate_queued_candidate_status_observer(&context, snapshot);
	}
	if !paused {
		paused = post_review::hydrate_post_review_lane_status_observer(&context, snapshot)?;
	}
	if paused {
		if snapshot.post_review_lanes.is_empty() {
			snapshot.post_review_lanes = status::build_degraded_post_review_lane_statuses(
				context.project,
				context.state_store,
				context.review_state_inspector,
			)?;
		}

		backoff::add_operator_snapshot_warning(snapshot, "external_observer_status_skipped");
	}

	Ok(())
}
