use crate::orchestrator::{
	self, ProjectDaemonRuntime, ProjectRegistration, StateStore,
	entrypoints_control_plane::{
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		project_tick::snapshots::context_failure,
		snapshot::{self, ControlPlaneProjectTick},
		status,
	},
};

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn control_plane_project_deferred_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
) -> ControlPlaneProjectTick {
	match orchestrator::load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache)
	{
		Ok(context) => match snapshot::build_operator_state_snapshot_without_live_observers(
			&context.config,
			&context.workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
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
					"Deferred control-plane snapshot build failed; sensitive runtime details were withheld."
				);

				ControlPlaneProjectTick {
					snapshot: None,
					project_status: Some(status::operator_project_status_from_registration(
						project, 1,
					)),
				}
			},
		},
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				"Deferred control-plane snapshot context failed; sensitive runtime details were withheld."
			);

			context_failure::control_plane_tick_context_failed_tick(project, &error, 1)
		},
	}
}
