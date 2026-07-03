use std::path::Path;

use crate::{
	orchestrator::status::{
		self, AccountActivityMode, GhPullRequestReviewStateInspector, IssueTracker,
		LiveOperatorStatusObserverContext, LiveOperatorStatusSnapshotOptions,
		OperatorStatusSnapshot, RunIssueMetadataHydration, ServiceConfig, StateStore,
		WorkflowDocument,
	},
	prelude::Result,
};

pub(crate) fn build_live_operator_status_snapshot<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	build_live_operator_status_snapshot_with_history_ledger(
		tracker,
		project,
		workflow,
		state_store,
		limit,
		LiveOperatorStatusSnapshotOptions {
			hydrate_history_ledger: true,
			run_issue_metadata_hydration: RunIssueMetadataHydration::AllRows,
			account_activity_mode: AccountActivityMode::Probe,
			configure_dispatch_slots: true,
		},
	)
}

pub(crate) fn build_status_command_operator_status_snapshot<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	build_live_operator_status_snapshot_with_history_ledger(
		tracker,
		project,
		workflow,
		state_store,
		limit,
		LiveOperatorStatusSnapshotOptions {
			hydrate_history_ledger: true,
			run_issue_metadata_hydration: RunIssueMetadataHydration::AllRows,
			account_activity_mode: AccountActivityMode::Snapshot,
			configure_dispatch_slots: true,
		},
	)
}

pub(crate) fn build_control_plane_operator_status_snapshot<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	build_live_operator_status_snapshot_with_history_ledger(
		tracker,
		project,
		workflow,
		state_store,
		limit,
		LiveOperatorStatusSnapshotOptions {
			hydrate_history_ledger: false,
			run_issue_metadata_hydration: RunIssueMetadataHydration::CurrentLaneRowsOnly,
			account_activity_mode: AccountActivityMode::Snapshot,
			configure_dispatch_slots: true,
		},
	)
}

pub(crate) fn build_live_operator_status_snapshot_with_history_ledger<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
	options: LiveOperatorStatusSnapshotOptions,
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	if options.configure_dispatch_slots {
		state_store.configure_dispatch_slot_root(project.service_id(), project.worktree_root())?;
	}

	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let execution_program_readback =
		status::operator_execution_program_statuses(tracker, project, workflow, state_store)?;
	let mut snapshot = status::build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		options.account_activity_mode,
	)?;

	snapshot.execution_programs = execution_program_readback.statuses;

	if execution_program_readback.issue_metadata_unavailable {
		status::add_operator_snapshot_warning(
			&mut snapshot,
			"execution_program_issue_metadata_unavailable",
		);
	}

	status::hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;
	status::hydrate_live_operator_external_observers(
		LiveOperatorStatusObserverContext {
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector: &review_state_inspector,
			hydrate_history_ledger: options.hydrate_history_ledger,
			run_issue_metadata_hydration: options.run_issue_metadata_hydration,
		},
		&mut snapshot,
	)?;
	status::apply_missing_issue_ghost_lane_projection(project, state_store, &mut snapshot)?;

	let terminal_projection = status::current_lane_terminal_projection_from_local_ledger(
		project,
		state_store,
		&snapshot,
	)?;

	status::apply_operator_lane_terminal_projection(
		&mut snapshot,
		terminal_projection,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	status::suppress_terminal_attention_queue_echoes(&mut snapshot);
	status::hydrate_post_review_lane_current_lane_shadowing(&mut snapshot);
	status::refresh_worktree_ownership(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	status::refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	Ok(snapshot)
}
