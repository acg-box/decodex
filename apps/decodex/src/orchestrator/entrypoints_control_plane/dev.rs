use crate::{
	orchestrator::{
		self, DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT, OperatorStatusSnapshot, ProjectRegistration,
		ServiceConfig, StateStore,
		entrypoints_control_plane::{snapshot, status},
	},
	prelude::Result,
};

pub(crate) fn run_control_plane_dev_tick(
	state_store: &StateStore,
) -> Result<OperatorStatusSnapshot> {
	let registered_projects = state_store.list_projects()?;
	let mut snapshot = super::empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);
	let mut project_statuses = Vec::new();

	if !registered_projects.iter().any(ProjectRegistration::enabled) {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, "no_enabled_projects");
	}

	orchestrator::add_operator_snapshot_warning(&mut snapshot, "automation_disabled");

	for registration in &registered_projects {
		let mut project_status =
			status::operator_project_status_from_dev_registration(registration);

		if registration.enabled() {
			match ServiceConfig::from_path(registration.config_path()).and_then(|project| {
				orchestrator::build_operator_status_snapshot(
					&project,
					state_store,
					DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
				)
			}) {
				Ok(project_snapshot) => {
					snapshot::hydrate_project_status_from_local_snapshot(
						&mut project_status,
						&project_snapshot,
					);
					snapshot::append_control_plane_project_snapshot(
						&mut snapshot,
						project_snapshot,
					);
				},
				Err(error) => {
					let _ = error;

					project_status.connector_state = String::from("config_error");
					project_status.warning_count = project_status.warning_count.saturating_add(1);

					orchestrator::add_operator_snapshot_warning(
						&mut snapshot,
						"operator_snapshot_build_failed",
					);

					tracing::warn!(
						project_id = registration.service_id(),
						"Dev operator snapshot local run hydration failed; sensitive runtime details were withheld."
					);
				},
			}
		} else {
			let mut project_warnings = Vec::new();
			let project_tick = snapshot::control_plane_disabled_project_observer_tick(
				registration,
				state_store,
				&mut project_warnings,
			);

			for warning in project_warnings {
				orchestrator::add_operator_snapshot_warning(&mut snapshot, warning);
			}

			if let Some(local_status) = project_tick.project_status {
				project_status =
					status::operator_project_status_from_dev_registration(registration);

				snapshot::hydrate_project_status_from_registered_status(
					&mut project_status,
					&local_status,
				);
			}
			if let Some(project_snapshot) = project_tick.snapshot {
				snapshot::append_control_plane_project_snapshot(&mut snapshot, project_snapshot);
			}
		}

		project_statuses.push(project_status);
	}

	snapshot.projects = project_statuses;
	snapshot.account_control = orchestrator::global_codex_account_control_status();

	Ok(snapshot)
}
