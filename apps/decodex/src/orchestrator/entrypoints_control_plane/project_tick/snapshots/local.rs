use crate::orchestrator::{
	self, OperatorConnectorBackoffStatus, ProjectDaemonRuntime, ProjectRegistration, StateStore,
	entrypoints_control_plane::{
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		project_tick::snapshots::context_failure,
		snapshot::{self, ControlPlaneProjectTick},
		status,
	},
};

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn control_plane_project_local_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	snapshot_warnings: &mut Vec<&'static str>,
	connector_backoffs: &[OperatorConnectorBackoffStatus],
) -> ControlPlaneProjectTick {
	match orchestrator::load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache)
	{
		Ok(context) => match orchestrator::build_operator_state_snapshot_for_publish(
			&context.tracker,
			&context.config,
			&context.workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
			snapshot_warnings,
			connector_backoffs,
		) {
			Ok(snapshot) => {
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
				let _ = error;

				tracing::warn!(
					project_id = project.service_id(),
					"Local operator snapshot build failed; sensitive runtime details were withheld."
				);

				snapshot_warnings.push("operator_snapshot_build_failed");
				ControlPlaneProjectTick {
					snapshot: None,
					project_status: Some(status::operator_project_status_from_registration(
						project,
						snapshot_warnings.len(),
					)),
				}
			},
		},
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				"Control-plane local snapshot context failed; sensitive runtime details were withheld."
			);

			context_failure::control_plane_tick_context_failed_tick(
				project,
				&error,
				snapshot_warnings.len() + 1,
			)
		},
	}
}
