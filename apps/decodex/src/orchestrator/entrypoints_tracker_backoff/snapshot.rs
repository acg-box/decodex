use crate::{
	orchestrator::{
		self, AccountActivityMode, GhPullRequestReviewStateInspector,
		OperatorConnectorBackoffStatus, OperatorStatusSnapshot, ServiceConfig, StateStore,
	},
	prelude::Result,
};

pub(crate) fn build_operator_status_snapshot_for_tracker_backoff(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
	status: &OperatorConnectorBackoffStatus,
) -> Result<OperatorStatusSnapshot> {
	let review_state_inspector = GhPullRequestReviewStateInspector::for_project(project);
	let mut snapshot = orchestrator::build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	orchestrator::hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;

	snapshot.post_review_lanes = orchestrator::build_degraded_post_review_lane_statuses(
		project,
		state_store,
		&review_state_inspector,
	)?;

	orchestrator::add_operator_snapshot_warning(&mut snapshot, &status.warning);

	snapshot.connector_backoffs.push(status.clone());

	orchestrator::add_operator_snapshot_warning(&mut snapshot, "external_observer_status_skipped");
	orchestrator::apply_terminal_history_ledger_outcomes(&mut snapshot);
	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}
