mod dev;
mod diagnose;
mod maintenance;
mod project_tick;
mod snapshot;
mod status;
mod tick;

#[cfg(test)]
pub(crate) use self::{dev::run_control_plane_dev_tick, tick::run_control_plane_tick};
pub(crate) use diagnose::{
	build_diagnose_live_snapshot, runtime_recovery_error_class, runtime_recovery_warning,
};
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

use crate::{
	orchestrator::{
		self, DEFAULT_CONTROL_PLANE_POLL_INTERVAL, DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		LINEAR_CONTROL_PLANE_POLL_INTERVAL, OPERATOR_DASHBOARD_WS_ENDPOINT_PATH,
		OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL, OPERATOR_LINEAR_SCAN_ENDPOINT_PATH,
		OperatorStateEndpoint, ProjectDaemonRuntime, ServeRequest,
	},
	prelude::{Result, eyre},
	runtime,
};

pub(crate) fn run_control_plane(request: ServeRequest<'_>) -> Result<()> {
	if request.dev && request.config_path.is_some() {
		eyre::bail!(
			"serve --dev does not accept --config because it must not register or poll projects."
		);
	}

	orchestrator::validate_daemon_runtime()?;

	let state_store = Arc::new(runtime::open_runtime_store()?);

	maintenance::run_control_plane_maintenance("startup");

	if request.dev {
		let operator_state_endpoint =
			OperatorStateEndpoint::start(request.listen_address, Arc::clone(&state_store))?;
		let runtime_db_path = runtime::runtime_db_path()?;
		let global_config_path = runtime::global_config_path()?;
		let project_config_dir = runtime::project_config_dir()?;

		tracing::info!(
			listen_address = %operator_state_endpoint.listen_address(),
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
			let snapshot = dev::run_control_plane_dev_tick(&state_store)?;

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
			maintenance::run_control_plane_maintenance("scheduled");

			next_maintenance_at = tick_started_at + Duration::from_secs(60 * 60);
		}

		let linear_scan_requests =
			tick::drain_operator_linear_scan_requests_best_effort(&operator_state_endpoint);
		let snapshot = tick::run_control_plane_tick_with_options(
			&state_store,
			&mut project_runtimes,
			&linear_scan_requests,
		)?;

		orchestrator::publish_operator_snapshot(&operator_state_endpoint, &snapshot);
		orchestrator::sleep_until_next_tick(DEFAULT_CONTROL_PLANE_POLL_INTERVAL, tick_started_at);
	}
}
