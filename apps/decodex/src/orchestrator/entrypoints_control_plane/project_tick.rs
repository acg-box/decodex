mod backoff;
mod schedule;
mod snapshots;

use std::{slice, time::Instant};

use crate::orchestrator::{
	self, OperatorLinearScanRequest, ProjectDaemonRuntime, ProjectRegistration, StateStore,
	entrypoints_control_plane::snapshot::ControlPlaneProjectTick,
};

pub(crate) fn run_control_plane_project_tick(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	snapshot_warnings: &mut Vec<&'static str>,
	linear_scan_requests: &[OperatorLinearScanRequest],
	now: Instant,
) -> ControlPlaneProjectTick {
	if backoff::tracker_backoff_active(runtime, now) {
		let connector_backoffs =
			orchestrator::active_connector_backoff_statuses(project.service_id(), runtime);

		for connector_backoff in &connector_backoffs {
			orchestrator::push_connector_backoff_warning(snapshot_warnings, connector_backoff);
		}

		return snapshots::control_plane_project_local_snapshot(
			project,
			state_store,
			runtime,
			snapshot_warnings,
			&connector_backoffs,
		);
	}

	if let Some(connector_backoff) = orchestrator::active_stored_tracker_backoff_status_best_effort(
		state_store,
		project.service_id(),
	) {
		orchestrator::push_connector_backoff_warning(snapshot_warnings, &connector_backoff);

		return snapshots::control_plane_project_local_snapshot(
			project,
			state_store,
			runtime,
			snapshot_warnings,
			slice::from_ref(&connector_backoff),
		);
	}

	if !schedule::linear_scan_due(project.service_id(), runtime, linear_scan_requests, now) {
		return snapshots::control_plane_project_deferred_snapshot(project, state_store, runtime);
	}

	schedule::remember_next_linear_scan(runtime, now);

	match orchestrator::load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache)
	{
		Ok(context) => snapshots::control_plane_project_snapshot(
			project,
			state_store,
			runtime,
			&context,
			snapshot_warnings,
		),
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				"Control-plane tick context failed; sensitive runtime details were withheld."
			);

			snapshots::control_plane_tick_context_failed_tick(project, &error, 1)
		},
	}
}

#[cfg(test)]
pub(crate) fn linear_scan_due(
	project_id: &str,
	runtime: &ProjectDaemonRuntime,
	linear_scan_requests: &[OperatorLinearScanRequest],
	now: Instant,
) -> bool {
	schedule::linear_scan_due(project_id, runtime, linear_scan_requests, now)
}

#[cfg(test)]
pub(crate) fn remember_next_linear_scan(runtime: &mut ProjectDaemonRuntime, now: Instant) {
	schedule::remember_next_linear_scan(runtime, now);
}
