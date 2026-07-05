use std::time::Instant;

use crate::{
	orchestrator::status::{
		self, IssueTracker, LiveOperatorStatusObserverContext, OperatorStatusSnapshot,
		snapshot::observers::backoff,
	},
	prelude::Result,
};

pub(crate) fn hydrate_post_review_lane_status_observer<T>(
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
			let Some(connector_backoff) = status::tracker_connector_backoff(
				&error,
				Instant::now(),
				"post_review_lane_status",
			) else {
				let _ = error;

				tracing::warn!(
					"Skipped post-review lane status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				backoff::add_operator_snapshot_warning(
					snapshot,
					"post_review_lane_status_unavailable",
				);

				return Ok(false);
			};

			backoff::pause_operator_snapshot_for_tracker_backoff(
				snapshot,
				context.state_store,
				context.project,
				&connector_backoff,
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
