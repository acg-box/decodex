use std::{slice, time::Instant};

use crate::orchestrator::{
	self, DaemonTickContext, ProjectDaemonRuntime, ProjectRegistration, StateStore,
	entrypoints_control_plane::{
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		project_tick::{
			backoff,
			snapshots::{context_failure, local},
		},
		snapshot::{self, ControlPlaneProjectTick},
		status,
	},
};

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn control_plane_project_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	context: &DaemonTickContext,
	snapshot_warnings: &mut Vec<&'static str>,
) -> ControlPlaneProjectTick {
	if let Err(error) = orchestrator::run_daemon_tick(
		project.config_path(),
		state_store,
		&mut runtime.active_children,
		&mut runtime.retry_queue,
		&mut runtime.recoverable_worktree_skip_cache,
		context,
	) {
		if let Some(connector_backoff) = backoff::remember_tracker_backoff(
			runtime,
			state_store,
			project.service_id(),
			&error,
			Instant::now(),
			"control_plane_tick",
		) {
			orchestrator::push_connector_backoff_warning(snapshot_warnings, &connector_backoff);

			return local::control_plane_project_local_snapshot(
				project,
				state_store,
				runtime,
				snapshot_warnings,
				slice::from_ref(&connector_backoff),
			);
		}

		let _ = error;

		tracing::warn!(
			project_id = project.service_id(),
			"Control-plane project tick failed; sensitive runtime details were withheld."
		);

		snapshot_warnings.push("control_plane_tick_failed");
	}

	match orchestrator::build_operator_state_snapshot_for_publish(
		&context.tracker,
		&context.config,
		&context.workflow,
		state_store,
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		snapshot_warnings,
		&[],
	) {
		Ok(snapshot) => {
			if !context_failure::operator_snapshot_has_linear_backoff(&snapshot) {
				runtime.tracker_backoff = None;

				orchestrator::clear_tracker_backoff_state_best_effort(
					state_store,
					project.service_id(),
				);
			}

			snapshot::write_snapshot_evidence(&snapshot);

			ControlPlaneProjectTick {
				project_status: snapshot
					.projects
					.first()
					.cloned()
					.map(|status| snapshot::complete_project_status(project, status)),
				snapshot: Some(snapshot),
			}
		},
		Err(error) => {
			if let Some(connector_backoff) = backoff::remember_tracker_backoff(
				runtime,
				state_store,
				project.service_id(),
				&error,
				Instant::now(),
				"operator_snapshot_refresh",
			) {
				orchestrator::push_connector_backoff_warning(snapshot_warnings, &connector_backoff);

				return local::control_plane_project_local_snapshot(
					project,
					state_store,
					runtime,
					snapshot_warnings,
					slice::from_ref(&connector_backoff),
				);
			}

			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Operator snapshot build failed; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("operator_snapshot_build_failed");

			ControlPlaneProjectTick {
				snapshot: None,
				project_status: Some(status::operator_project_status_from_registration(project, 1)),
			}
		},
	}
}
