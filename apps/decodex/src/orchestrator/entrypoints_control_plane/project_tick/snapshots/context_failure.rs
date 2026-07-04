use color_eyre::Report;

use crate::orchestrator::{
	self, OperatorSnapshotWarningDetail, OperatorStatusSnapshot, ProjectRegistration,
	entrypoints_control_plane::{
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT, snapshot::ControlPlaneProjectTick, status,
	},
};

const CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING: &str = "control_plane_tick_context_failed";

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn operator_snapshot_has_linear_backoff(
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot.connector_backoffs.iter().any(|backoff| backoff.connector == "linear")
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn control_plane_tick_context_failed_tick(
	project: &ProjectRegistration,
	error: &Report,
	warning_count: usize,
) -> ControlPlaneProjectTick {
	let mut snapshot = status::empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);

	orchestrator::add_operator_snapshot_warning(
		&mut snapshot,
		CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING,
	);

	snapshot.warning_details.push(control_plane_tick_context_failed_warning_detail(project, error));

	ControlPlaneProjectTick {
		snapshot: Some(snapshot),
		project_status: Some(status::operator_project_status_from_registration(
			project,
			warning_count,
		)),
	}
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn control_plane_tick_context_failed_warning_detail(
	project: &ProjectRegistration,
	error: &Report,
) -> OperatorSnapshotWarningDetail {
	let error_message = error.to_string();
	let (reason, next_action) =
		control_plane_tick_context_failed_warning_text(project, &error_message);

	OperatorSnapshotWarningDetail {
		warning: String::from(CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING),
		project_id: Some(project.service_id().to_owned()),
		repo_root: Some(project.repo_root().display().to_string()),
		reason,
		next_action: Some(next_action),
	}
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn control_plane_tick_context_failed_warning_text(
	project: &ProjectRegistration,
	error_message: &str,
) -> (String, String) {
	if let Some(env_var) = context_failure_env_var(error_message) {
		return (
			format!(
				"Control-plane context could not read configured environment variable `{env_var}` for `{}`.",
				project.config_path().display()
			),
			format!(
				"Expose `{env_var}` to the `decodex serve` process, then restart the Decodex App/helper. For macOS GUI launches, set it with `launchctl setenv {env_var} <value>`."
			),
		);
	}

	if error_message.contains("WORKFLOW.md") {
		return (
			format!(
				"Control-plane context could not load the project workflow for `{}`: {error_message}",
				project.config_path().display()
			),
			String::from(
				"Restore or fix the registered project `WORKFLOW.md`, then request a Linear scan or restart the control plane.",
			),
		);
	}

	(
		format!(
			"Control-plane context could not load registered project `{}`: {error_message}",
			project.config_path().display()
		),
		String::from(
			"Inspect the registered `project.toml`, `WORKFLOW.md`, and configured credential environment, then restart or rescan the control plane.",
		),
	)
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn context_failure_env_var(
	error_message: &str,
) -> Option<String> {
	error_message
		.split("environment variable `")
		.nth(1)?
		.split('`')
		.next()
		.filter(|env_var| !env_var.is_empty())
		.map(str::to_owned)
}
