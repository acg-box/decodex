mod project_tick;
mod snapshot;
mod status;

#[cfg(test)] pub(crate) use project_tick::{linear_scan_due, remember_next_linear_scan};
pub(crate) use snapshot::build_operator_state_snapshot_without_live_observers;
#[cfg(test)]
pub(crate) use snapshot::{
	ControlPlaneProjectTick, collect_control_plane_snapshot, complete_project_status,
};
pub(crate) use status::empty_control_plane_snapshot;

use std::{
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use color_eyre::Report;

use crate::{
	maintenance,
	orchestrator::{
		self, DEFAULT_CONTROL_PLANE_POLL_INTERVAL, DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		LINEAR_CONTROL_PLANE_POLL_INTERVAL, OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH,
		OPERATOR_DASHBOARD_WS_ENDPOINT_PATH, OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL,
		OPERATOR_LINEAR_SCAN_ENDPOINT_PATH, OperatorLinearScanRequest, OperatorStateEndpoint,
		OperatorStatusSnapshot, ProjectDaemonRuntime, ProjectRegistration, ServeRequest,
		ServiceConfig, StateStore, WorkflowDocument,
	},
	prelude::{Result, eyre},
	runtime,
	tracker::IssueTracker,
};

pub(crate) fn run_control_plane(request: ServeRequest<'_>) -> Result<()> {
	if request.dev && request.config_path.is_some() {
		eyre::bail!(
			"serve --dev does not accept --config because it must not register or poll projects."
		);
	}

	orchestrator::validate_daemon_runtime()?;

	let state_store = Arc::new(runtime::open_runtime_store()?);

	run_control_plane_maintenance("startup");

	if request.dev {
		let operator_state_endpoint =
			OperatorStateEndpoint::start(request.listen_address, Arc::clone(&state_store))?;
		let runtime_db_path = runtime::runtime_db_path()?;
		let global_config_path = runtime::global_config_path()?;
		let project_config_dir = runtime::project_config_dir()?;

		tracing::info!(
			listen_address = %operator_state_endpoint.listen_address(),
			path = OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH,
			ws_path = OPERATOR_DASHBOARD_WS_ENDPOINT_PATH,
			dev = true,
			stream_interval_s = OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL.as_secs(),
			runtime_db_path = %runtime_db_path.display(),
			global_config_path = %global_config_path.display(),
			project_config_dir = %project_config_dir.display(),
			"Starting Decodex dev operator endpoint."
		);

		loop {
			let tick_started_at = Instant::now();
			let snapshot = run_control_plane_dev_tick(&state_store)?;

			orchestrator::publish_operator_snapshot(&operator_state_endpoint, &snapshot);
			orchestrator::sleep_until_next_tick(
				OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL,
				tick_started_at,
			);
		}
	}

	if let Some(config_path) = request.config_path {
		let Some(config_path) = orchestrator::resolve_config_path(Some(config_path), &state_store)?
		else {
			eyre::bail!(
				"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
			);
		};

		runtime::register_project_config(&state_store, &config_path, true)?;
	}

	let operator_state_endpoint =
		OperatorStateEndpoint::start(request.listen_address, Arc::clone(&state_store))?;
	let runtime_db_path = runtime::runtime_db_path()?;
	let global_config_path = runtime::global_config_path()?;
	let project_config_dir = runtime::project_config_dir()?;
	let mut project_runtimes: HashMap<String, ProjectDaemonRuntime> = HashMap::new();
	let mut next_maintenance_at = Instant::now() + Duration::from_secs(60 * 60);

	tracing::info!(
		local_tick_interval_s = DEFAULT_CONTROL_PLANE_POLL_INTERVAL.as_secs(),
		linear_poll_interval_s = LINEAR_CONTROL_PLANE_POLL_INTERVAL.as_secs(),
		listen_address = %operator_state_endpoint.listen_address(),
		path = OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH,
		ws_path = OPERATOR_DASHBOARD_WS_ENDPOINT_PATH,
		linear_scan_path = OPERATOR_LINEAR_SCAN_ENDPOINT_PATH,
		dev = false,
		runtime_db_path = %runtime_db_path.display(),
		global_config_path = %global_config_path.display(),
		project_config_dir = %project_config_dir.display(),
		"Starting Decodex control-plane poll loop."
	);

	loop {
		let tick_started_at = Instant::now();

		if tick_started_at >= next_maintenance_at {
			run_control_plane_maintenance("scheduled");

			next_maintenance_at = tick_started_at + Duration::from_secs(60 * 60);
		}

		let linear_scan_requests =
			drain_operator_linear_scan_requests_best_effort(&operator_state_endpoint);
		let snapshot = run_control_plane_tick_with_options(
			&state_store,
			&mut project_runtimes,
			&linear_scan_requests,
		)?;

		orchestrator::publish_operator_snapshot(&operator_state_endpoint, &snapshot);
		orchestrator::sleep_until_next_tick(DEFAULT_CONTROL_PLANE_POLL_INTERVAL, tick_started_at);
	}
}

pub(crate) fn runtime_recovery_warning(prefix: &str, error: &Report) -> String {
	format!("{prefix}:{}", runtime_recovery_error_class(error))
}

pub(crate) fn runtime_recovery_error_class(error: &Report) -> &'static str {
	let message = error.to_string().to_ascii_lowercase();

	if message.contains("linear") || message.contains("tracker") {
		return "tracker";
	}
	if message.contains("worktree") || message.contains("work tree") {
		return "worktree";
	}
	if message.contains("runtime") || message.contains("sqlite") || message.contains("database") {
		return "runtime_store";
	}

	"unknown"
}

pub(crate) fn build_diagnose_live_snapshot<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	let mut snapshot_warnings = Vec::new();

	match orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		tracker,
		config,
		workflow,
		state_store,
	) {
		Ok(recovered_state) =>
			orchestrator::hydrate_status_snapshot_state(config, state_store, recovered_state)?,
		Err(error) => {
			let warning = runtime_recovery_warning("diagnose_runtime_recovery_unavailable", &error);

			tracing::warn!(
				project_id = config.service_id(),
				recovery_error_class = runtime_recovery_error_class(&error),
				"Skipped runtime recovery for diagnose; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(warning);
		},
	}

	let mut snapshot = match orchestrator::build_live_operator_status_snapshot(
		tracker,
		config,
		workflow,
		state_store,
		limit,
	) {
		Ok(snapshot) => snapshot,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = config.service_id(),
				"Fell back to local diagnose snapshot; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(String::from("diagnose_live_observer_unavailable"));

			orchestrator::build_operator_status_snapshot(config, state_store, limit)?
		},
	};

	for warning in snapshot_warnings {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, &warning);
	}

	Ok(snapshot)
}

#[cfg(test)]
pub(crate) fn run_control_plane_tick(
	state_store: &StateStore,
	project_runtimes: &mut HashMap<String, ProjectDaemonRuntime>,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> Result<OperatorStatusSnapshot> {
	run_control_plane_tick_with_options(state_store, project_runtimes, linear_scan_requests)
}

pub(crate) fn run_control_plane_dev_tick(
	state_store: &StateStore,
) -> Result<OperatorStatusSnapshot> {
	let registered_projects = state_store.list_projects()?;
	let mut snapshot = empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);
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

fn run_control_plane_maintenance(trigger: &'static str) {
	match maintenance::run_auto_safe_prune() {
		Ok(report) => {
			tracing::info!(
				trigger = trigger,
				log_rotated_files = report.logs.rotated_files,
				evidence_rotated_files = report.agent_evidence.rotated_files,
				backup_deleted_files = report.backups.deleted_files,
				wal_checkpoint_mode = report
					.wal_checkpoint
					.as_ref()
					.map(|checkpoint| checkpoint.mode)
					.unwrap_or("skipped"),
				"Completed Decodex auto-safe maintenance."
			);
		},
		Err(error) => {
			let _ = error;

			tracing::warn!(
				trigger = trigger,
				"Decodex auto-safe maintenance failed; sensitive runtime details were withheld from control-plane logs."
			);
		},
	}
}

fn run_control_plane_tick_with_options(
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

fn drain_operator_linear_scan_requests_best_effort(
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
