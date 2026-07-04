use std::{collections::HashMap, time::Instant};

use crate::{
	orchestrator::{
		OperatorLinearScanRequest, OperatorStateEndpoint, OperatorStatusSnapshot,
		ProjectDaemonRuntime, StateStore,
		entrypoints_control_plane::{project_tick, snapshot},
	},
	prelude::Result,
};

#[cfg(test)]
pub(crate) fn run_control_plane_tick(
	state_store: &StateStore,
	project_runtimes: &mut HashMap<String, ProjectDaemonRuntime>,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> Result<OperatorStatusSnapshot> {
	run_control_plane_tick_with_options(state_store, project_runtimes, linear_scan_requests)
}

pub(crate) fn run_control_plane_tick_with_options(
	state_store: &StateStore,
	project_runtimes: &mut HashMap<String, ProjectDaemonRuntime>,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> Result<OperatorStatusSnapshot> {
	let registered_projects = state_store.list_projects()?;
	let now = Instant::now();

	Ok(snapshot::collect_control_plane_snapshot(
		registered_projects,
		|project, project_warnings| {
			if project.enabled() {
				let runtime = project_runtimes.entry(project.service_id().to_owned()).or_default();

				project_tick::run_control_plane_project_tick(
					project,
					state_store,
					runtime,
					project_warnings,
					linear_scan_requests,
					now,
				)
			} else {
				snapshot::control_plane_disabled_project_observer_tick(
					project,
					state_store,
					project_warnings,
				)
			}
		},
	))
}

pub(crate) fn drain_operator_linear_scan_requests_best_effort(
	operator_state_endpoint: &OperatorStateEndpoint,
) -> Vec<OperatorLinearScanRequest> {
	match operator_state_endpoint.drain_linear_scan_requests() {
		Ok(requests) => requests,
		Err(error) => {
			tracing::warn!(?error, "Skipped operator-triggered Linear scan requests.");

			Vec::new()
		},
	}
}
