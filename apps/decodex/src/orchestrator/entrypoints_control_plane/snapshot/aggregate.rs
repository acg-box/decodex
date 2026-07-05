use crate::orchestrator::{
	self, OperatorStatusSnapshot, ProjectRegistration,
	entrypoints_control_plane::{
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT, snapshot::ControlPlaneProjectTick, status,
	},
};

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
