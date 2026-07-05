use crate::{
	orchestrator::{
		self, AccountActivityMode, OperatorStatusSnapshot, ProjectRegistration, ServiceConfig,
		StateStore, WorkflowDocument,
		entrypoints_control_plane::DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
	},
	prelude::Result,
};

pub(crate) fn build_operator_state_snapshot_without_live_observers(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	state_store.configure_dispatch_slot_root(project.service_id(), project.worktree_root())?;

	let mut snapshot = orchestrator::build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	orchestrator::hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;

	let terminal_projection = orchestrator::current_lane_terminal_projection_from_local_ledger(
		project,
		state_store,
		&snapshot,
	)?;

	orchestrator::apply_operator_lane_terminal_projection(
		&mut snapshot,
		terminal_projection,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	orchestrator::refresh_worktree_ownership(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	orchestrator::refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	Ok(snapshot)
}

pub(in crate::orchestrator::entrypoints_control_plane::snapshot) fn build_registered_project_local_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
) -> Result<OperatorStatusSnapshot> {
	let config = ServiceConfig::from_path(project.config_path())?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		state_store,
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
	)
}
