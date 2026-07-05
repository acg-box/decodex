use crate::orchestrator::{
	OperatorProjectStatus, OperatorStatusSnapshot, ProjectRegistration, StateStore,
	entrypoints_control_plane::{
		snapshot::{ControlPlaneProjectTick, local, project_status},
		status,
	},
};

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

	match local::build_registered_project_local_snapshot(project, state_store) {
		Ok(project_snapshot) => build_disabled_project_tick(project_status, project_snapshot),
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

fn build_disabled_project_tick(
	mut project_status: OperatorProjectStatus,
	project_snapshot: OperatorStatusSnapshot,
) -> ControlPlaneProjectTick {
	project_status::hydrate_project_status_from_local_snapshot(
		&mut project_status,
		&project_snapshot,
	);

	ControlPlaneProjectTick {
		snapshot: Some(project_snapshot),
		project_status: Some(project_status),
	}
}
