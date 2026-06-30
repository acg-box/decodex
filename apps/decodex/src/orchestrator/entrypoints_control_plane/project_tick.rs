use std::{slice, time::Instant};

use color_eyre::Report;
use time::OffsetDateTime;

use super::super::{
	DaemonTickContext, LINEAR_CONTROL_PLANE_POLL_INTERVAL, OperatorConnectorBackoffStatus,
	OperatorLinearScanRequest, OperatorSnapshotWarningDetail, OperatorStatusSnapshot,
	ProjectDaemonRuntime, ProjectRegistration, StateStore, active_connector_backoff_statuses,
	active_stored_tracker_backoff_status_best_effort, add_operator_snapshot_warning,
	build_operator_state_snapshot_for_publish, clear_tracker_backoff_state_best_effort,
	load_daemon_tick_context, persist_tracker_backoff_state, push_connector_backoff_warning,
	run_daemon_tick, tracker_connector_backoff,
};
use super::DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT;
use super::snapshot::{
	ControlPlaneProjectTick, build_operator_state_snapshot_without_live_observers,
	complete_project_status, write_snapshot_evidence,
};
use super::status::{empty_control_plane_snapshot, operator_project_status_from_registration};

const CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING: &str = "control_plane_tick_context_failed";

pub(crate) fn run_control_plane_project_tick(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	snapshot_warnings: &mut Vec<&'static str>,
	linear_scan_requests: &[OperatorLinearScanRequest],
	now: Instant,
) -> ControlPlaneProjectTick {
	if tracker_backoff_active(runtime, now) {
		let connector_backoffs = active_connector_backoff_statuses(project.service_id(), runtime);

		for connector_backoff in &connector_backoffs {
			push_connector_backoff_warning(snapshot_warnings, connector_backoff);
		}

		return control_plane_project_local_snapshot(
			project,
			state_store,
			runtime,
			snapshot_warnings,
			&connector_backoffs,
		);
	}

	if let Some(connector_backoff) =
		active_stored_tracker_backoff_status_best_effort(state_store, project.service_id())
	{
		push_connector_backoff_warning(snapshot_warnings, &connector_backoff);

		return control_plane_project_local_snapshot(
			project,
			state_store,
			runtime,
			snapshot_warnings,
			slice::from_ref(&connector_backoff),
		);
	}

	if !linear_scan_due(project.service_id(), runtime, linear_scan_requests, now) {
		return control_plane_project_deferred_snapshot(project, state_store, runtime);
	}

	remember_next_linear_scan(runtime, now);

	match load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache) {
		Ok(context) => control_plane_project_snapshot(
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

			control_plane_tick_context_failed_tick(project, &error, 1)
		},
	}
}

fn tracker_backoff_active(runtime: &mut ProjectDaemonRuntime, now: Instant) -> bool {
	if runtime.tracker_backoff.as_ref().is_some_and(|backoff| backoff.until > now) {
		return true;
	}

	runtime.tracker_backoff = None;

	false
}

pub(crate) fn linear_scan_due(
	project_id: &str,
	runtime: &ProjectDaemonRuntime,
	linear_scan_requests: &[OperatorLinearScanRequest],
	now: Instant,
) -> bool {
	if linear_scan_requested(project_id, linear_scan_requests) {
		return true;
	}

	runtime.next_linear_scan_at.is_none_or(|next_scan_at| now >= next_scan_at)
}

fn linear_scan_requested(
	project_id: &str,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> bool {
	linear_scan_requests.iter().any(|request| {
		request
			.project_id
			.as_deref()
			.is_none_or(|requested_project_id| requested_project_id == project_id)
	})
}

pub(crate) fn remember_next_linear_scan(runtime: &mut ProjectDaemonRuntime, now: Instant) {
	runtime.next_linear_scan_at = Some(now + LINEAR_CONTROL_PLANE_POLL_INTERVAL);
}

fn remember_tracker_backoff(
	runtime: &mut ProjectDaemonRuntime,
	state_store: &StateStore,
	project_id: &str,
	error: &Report,
	now: Instant,
	sync_phase: &'static str,
) -> Option<OperatorConnectorBackoffStatus> {
	let backoff = tracker_connector_backoff(error, now, sync_phase)?;
	let status = backoff.to_operator_status(project_id, OffsetDateTime::now_utc().unix_timestamp());

	persist_tracker_backoff_state(state_store, project_id, &backoff);

	runtime.tracker_backoff = Some(backoff);

	Some(status)
}

fn control_plane_project_deferred_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
) -> ControlPlaneProjectTick {
	match load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache) {
		Ok(context) => match build_operator_state_snapshot_without_live_observers(
			&context.config,
			&context.workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		) {
			Ok(snapshot) => {
				write_snapshot_evidence(&snapshot);

				ControlPlaneProjectTick {
					project_status: snapshot
						.projects
						.first()
						.cloned()
						.map(|status| complete_project_status(project, status)),
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
					project_status: Some(operator_project_status_from_registration(project, 1)),
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

fn control_plane_project_local_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	snapshot_warnings: &mut Vec<&'static str>,
	connector_backoffs: &[OperatorConnectorBackoffStatus],
) -> ControlPlaneProjectTick {
	match load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache) {
		Ok(context) => match build_operator_state_snapshot_for_publish(
			&context.tracker,
			&context.config,
			&context.workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
			snapshot_warnings,
			connector_backoffs,
		) {
			Ok(snapshot) => {
				write_snapshot_evidence(&snapshot);

				ControlPlaneProjectTick {
					project_status: snapshot
						.projects
						.first()
						.cloned()
						.map(|status| complete_project_status(project, status)),
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
					project_status: Some(operator_project_status_from_registration(
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

fn control_plane_project_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	context: &DaemonTickContext,
	snapshot_warnings: &mut Vec<&'static str>,
) -> ControlPlaneProjectTick {
	if let Err(error) = run_daemon_tick(
		project.config_path(),
		state_store,
		&mut runtime.active_children,
		&mut runtime.retry_queue,
		&mut runtime.recoverable_worktree_skip_cache,
		context,
	) {
		if let Some(connector_backoff) = remember_tracker_backoff(
			runtime,
			state_store,
			project.service_id(),
			&error,
			Instant::now(),
			"control_plane_tick",
		) {
			push_connector_backoff_warning(snapshot_warnings, &connector_backoff);

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

	match build_operator_state_snapshot_for_publish(
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

				clear_tracker_backoff_state_best_effort(state_store, project.service_id());
			}

			write_snapshot_evidence(&snapshot);

			ControlPlaneProjectTick {
				project_status: snapshot
					.projects
					.first()
					.cloned()
					.map(|status| complete_project_status(project, status)),
				snapshot: Some(snapshot),
			}
		},
		Err(error) => {
			if let Some(connector_backoff) = remember_tracker_backoff(
				runtime,
				state_store,
				project.service_id(),
				&error,
				Instant::now(),
				"operator_snapshot_refresh",
			) {
				push_connector_backoff_warning(snapshot_warnings, &connector_backoff);

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
				project_status: Some(operator_project_status_from_registration(project, 1)),
			}
		},
	}
}

fn operator_snapshot_has_linear_backoff(snapshot: &OperatorStatusSnapshot) -> bool {
	snapshot.connector_backoffs.iter().any(|backoff| backoff.connector == "linear")
}

fn control_plane_tick_context_failed_tick(
	project: &ProjectRegistration,
	error: &Report,
	warning_count: usize,
) -> ControlPlaneProjectTick {
	let mut snapshot = empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);

	add_operator_snapshot_warning(&mut snapshot, CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING);

	snapshot.warning_details.push(control_plane_tick_context_failed_warning_detail(project, error));

	ControlPlaneProjectTick {
		snapshot: Some(snapshot),
		project_status: Some(operator_project_status_from_registration(project, warning_count)),
	}
}

fn control_plane_tick_context_failed_warning_detail(
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

fn control_plane_tick_context_failed_warning_text(
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

fn context_failure_env_var(error_message: &str) -> Option<String> {
	error_message
		.split("environment variable `")
		.nth(1)?
		.split('`')
		.next()
		.filter(|env_var| !env_var.is_empty())
		.map(str::to_owned)
}
