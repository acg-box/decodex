use crate::{
	orchestrator::{
		self, AccountActivityMode, AgentEvidenceSource, OperatorProjectStatus,
		OperatorStatusSnapshot, ProjectRegistration, ServiceConfig, StateStore, WorkflowDocument,
		entrypoints_control_plane::{DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT, status},
	},
	prelude::Result,
};

pub(crate) struct ControlPlaneProjectTick {
	pub(crate) snapshot: Option<OperatorStatusSnapshot>,
	pub(crate) project_status: Option<OperatorProjectStatus>,
}

pub(crate) fn collect_control_plane_snapshot<F>(
	registered_projects: Vec<ProjectRegistration>,
	mut run_project_tick: F,
) -> OperatorStatusSnapshot
where
	F: FnMut(&ProjectRegistration, &mut Vec<&'static str>) -> ControlPlaneProjectTick,
{
	let registered_project_count = registered_projects.len();
	let mut snapshot_warnings = Vec::new();
	let mut project_statuses = Vec::new();
	let mut project_snapshots = Vec::new();

	if !registered_projects.iter().any(ProjectRegistration::enabled) {
		snapshot_warnings.push("no_enabled_projects");
	}

	for project in registered_projects {
		let mut project_warnings = Vec::new();
		let project_tick = run_project_tick(&project, &mut project_warnings);

		snapshot_warnings.extend(project_warnings);

		if let Some(status) = project_tick.project_status {
			project_statuses.push(status);
		}
		if let Some(snapshot) = project_tick.snapshot {
			project_snapshots.push(snapshot);
		}
	}

	let mut snapshot =
		aggregate_control_plane_snapshot(registered_project_count, project_snapshots);

	snapshot.projects = project_statuses;
	snapshot.account_control = orchestrator::global_codex_account_control_status();

	for warning in snapshot_warnings {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, warning);
	}

	snapshot
}

pub(crate) fn control_plane_disabled_project_observer_tick(
	project: &ProjectRegistration,
	state_store: &StateStore,
	snapshot_warnings: &mut Vec<&'static str>,
) -> ControlPlaneProjectTick {
	let project_status = status::operator_project_status_from_registration(project, 0);
	let current_lanes = match state_store.list_leased_runs(project.service_id()) {
		Ok(current_lanes) => current_lanes,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Disabled project leased-run lookup failed; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("operator_snapshot_build_failed");

			return ControlPlaneProjectTick {
				snapshot: None,
				project_status: Some(project_status),
			};
		},
	};

	if current_lanes.is_empty() {
		return ControlPlaneProjectTick { snapshot: None, project_status: Some(project_status) };
	}

	match build_registered_project_local_snapshot(project, state_store) {
		Ok(project_snapshot) => {
			let mut project_status = project_status;

			hydrate_project_status_from_local_snapshot(&mut project_status, &project_snapshot);

			ControlPlaneProjectTick {
				snapshot: Some(project_snapshot),
				project_status: Some(project_status),
			}
		},
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Disabled project leased-run snapshot build failed; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("operator_snapshot_build_failed");

			ControlPlaneProjectTick { snapshot: None, project_status: Some(project_status) }
		},
	}
}

pub(crate) fn hydrate_project_status_from_local_snapshot(
	project_status: &mut OperatorProjectStatus,
	project_snapshot: &OperatorStatusSnapshot,
) {
	if let Some(local_status) = project_snapshot.projects.first() {
		hydrate_project_status_from_registered_status(project_status, local_status);
	} else {
		project_status.current_lane_count = project_snapshot.current_lanes.len();
		project_status.running_lane_count = project_snapshot
			.current_lanes
			.iter()
			.filter(|run| orchestrator::operator_run_counts_as_running(run))
			.count();
	}
}

pub(crate) fn hydrate_project_status_from_registered_status(
	project_status: &mut OperatorProjectStatus,
	local_status: &OperatorProjectStatus,
) {
	project_status.current_lane_count = local_status.current_lane_count;
	project_status.running_lane_count = local_status.running_lane_count;
	project_status.retained_worktree_count = local_status.retained_worktree_count;
	project_status.waiting_lane_count = local_status.waiting_lane_count;
	project_status.attention_count = local_status.attention_count;
	project_status.cleanup_blocked_count = local_status.cleanup_blocked_count;
	project_status.cleanup_pending_count = local_status.cleanup_pending_count;
	project_status.last_activity_at = local_status.last_activity_at.clone();
	project_status.warning_count =
		project_status.warning_count.saturating_add(local_status.warning_count);
}

pub(crate) fn append_control_plane_project_snapshot(
	snapshot: &mut OperatorStatusSnapshot,
	project_snapshot: OperatorStatusSnapshot,
) {
	for warning in project_snapshot.warnings {
		orchestrator::add_operator_snapshot_warning(snapshot, &warning);
	}

	snapshot.warning_details.extend(project_snapshot.warning_details);
	snapshot.connector_backoffs.extend(project_snapshot.connector_backoffs);
	snapshot.accounts.extend(project_snapshot.accounts);
	snapshot.current_lanes.extend(project_snapshot.current_lanes);
	snapshot.recent_runs.extend(project_snapshot.recent_runs);
	snapshot.history_lanes.extend(project_snapshot.history_lanes);
	snapshot.execution_programs.extend(project_snapshot.execution_programs);
	snapshot.queued_candidates.extend(project_snapshot.queued_candidates);
	snapshot.worktrees.extend(project_snapshot.worktrees);
	snapshot.post_review_lanes.extend(project_snapshot.post_review_lanes);
}

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

pub(crate) fn complete_project_status(
	project: &ProjectRegistration,
	mut status: OperatorProjectStatus,
) -> OperatorProjectStatus {
	status.config_path = project.config_path().display().to_string();
	status.enabled = project.enabled();

	status
}

pub(crate) fn write_snapshot_evidence(snapshot: &OperatorStatusSnapshot) {
	orchestrator::write_agent_evidence_best_effort(snapshot, AgentEvidenceSource::ServeTick);
}

fn build_registered_project_local_snapshot(
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

fn aggregate_control_plane_snapshot(
	registered_project_count: usize,
	mut project_snapshots: Vec<OperatorStatusSnapshot>,
) -> OperatorStatusSnapshot {
	if registered_project_count == 1 && project_snapshots.len() == 1 {
		return project_snapshots.remove(0);
	}

	let mut snapshot = status::empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);

	for project_snapshot in project_snapshots {
		append_control_plane_project_snapshot(&mut snapshot, project_snapshot);
	}

	snapshot
}
