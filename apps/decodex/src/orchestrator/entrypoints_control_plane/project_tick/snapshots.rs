use std::{slice, time::Instant};

use color_eyre::Report;

use crate::orchestrator::{
	self, DaemonTickContext, OperatorConnectorBackoffStatus, OperatorSnapshotWarningDetail,
	OperatorStatusSnapshot, ProjectDaemonRuntime, ProjectRegistration, StateStore,
	entrypoints_control_plane::{
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT, project_tick::backoff, snapshot,
		snapshot::ControlPlaneProjectTick, status,
	},
};

const CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING: &str = "control_plane_tick_context_failed";

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

			control_plane_tick_context_failed_tick(project, &error, 1)
		},
	}
}

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

			control_plane_tick_context_failed_tick(project, &error, snapshot_warnings.len() + 1)
		},
	}
}

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

			return control_plane_project_local_snapshot(
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
			if !operator_snapshot_has_linear_backoff(&snapshot) {
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

				return control_plane_project_local_snapshot(
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
