use crate::{
	lane_authority::LaneId,
	orchestrator::status::{
		RunActivityMarker, ServiceConfig, StateStore, TrackerIssue, WorktreeSpec,
	},
	prelude::Result,
};

pub(crate) fn upsert_recovered_worktree_mapping(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	worktree: &WorktreeSpec,
	activity_marker: Option<&RunActivityMarker>,
) -> Result<()> {
	state_store.upsert_recovered_worktree(
		project.service_id(),
		&issue.id,
		&worktree.branch_name,
		&worktree.path.display().to_string(),
		recovered_worktree_observed_at_unix(activity_marker),
	)
}

pub(crate) fn recovered_worktree_observed_at_unix(
	activity_marker: Option<&RunActivityMarker>,
) -> Option<i64> {
	activity_marker.and_then(|marker| {
		[
			marker.last_activity_unix_epoch(),
			marker.last_protocol_activity_unix_epoch(),
			marker.last_progress_unix_epoch(),
		]
		.into_iter()
		.flatten()
		.max()
	})
}

pub(crate) fn record_recovered_activity_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: &RunActivityMarker,
) -> Result<()> {
	let lane_id = LaneId::new(project.service_id(), &issue.id)?;
	let lane = state_store.lane(&lane_id)?.ok_or_else(|| {
		color_eyre::eyre::eyre!("Runtime recovery requires canonical lane authority.")
	})?;
	if lane.intake_authority_id().is_none() || lane.claim_run_id() != Some(marker.run_id()) {
		color_eyre::eyre::bail!("Runtime recovery marker does not match canonical lane authority.");
	}
	state_store.record_lane_run_attempt(
		project.service_id(),
		marker.run_id(),
		&issue.id,
		marker.attempt_number(),
		"running",
	)?;

	Ok(())
}
