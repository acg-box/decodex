use crate::orchestrator::{
	self, AccountActivityMode, GhPullRequestReviewStateInspector, IssueTracker,
	OperatorConnectorBackoffStatus, OperatorStatusSnapshot, Path, Result, ServiceConfig,
	StateStore, WorkflowDocument,
};

pub(crate) fn build_operator_state_snapshot_for_publish<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
	warnings: &[&str],
	connector_backoffs: &[OperatorConnectorBackoffStatus],
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	let mut snapshot = if warnings.is_empty() {
		orchestrator::build_control_plane_operator_status_snapshot(
			tracker,
			project,
			workflow,
			state_store,
			limit,
		)?
	} else {
		orchestrator::build_operator_status_snapshot_with_account_mode(
			project,
			state_store,
			limit,
			AccountActivityMode::Snapshot,
		)?
	};

	if !warnings.is_empty() {
		orchestrator::hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;
	}

	orchestrator::apply_terminal_history_ledger_outcomes(&mut snapshot);

	if orchestrator::warnings_include_tracker_backoff(warnings) {
		let review_state_inspector = GhPullRequestReviewStateInspector {
			github_token_env_var: Some(project.github().token_env_var().to_owned()),
			github_command_path: project.github().command_path().map(Path::to_path_buf),
		};

		snapshot.post_review_lanes = orchestrator::build_degraded_post_review_lane_statuses(
			project,
			state_store,
			&review_state_inspector,
		)?;
	}

	for warning in warnings {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, warning);
	}

	snapshot.connector_backoffs.extend(connector_backoffs.iter().cloned());

	if !warnings.is_empty() {
		orchestrator::add_operator_snapshot_warning(
			&mut snapshot,
			"external_observer_status_skipped",
		);
	}

	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}
