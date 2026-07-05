use std::time::Instant;

use crate::orchestrator::status::{
	self, IssueTracker, LiveOperatorStatusObserverContext, OperatorStatusSnapshot,
	snapshot::observers::backoff,
};

pub(crate) fn hydrate_queued_candidate_status_observer<T>(
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
			let Some(connector_backoff) = status::tracker_connector_backoff(
				&error,
				Instant::now(),
				"queued_candidate_status",
			) else {
				let _ = error;

				tracing::warn!(
					"Skipped queued candidate status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				backoff::add_operator_snapshot_warning(
					snapshot,
					"queued_candidate_status_unavailable",
				);

				return false;
			};

			backoff::pause_operator_snapshot_for_tracker_backoff(
				snapshot,
				context.state_store,
				context.project,
				&connector_backoff,
			);

			true
		},
	}
}
