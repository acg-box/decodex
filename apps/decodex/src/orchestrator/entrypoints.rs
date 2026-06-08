use state::{ConnectorBackoff, ConnectorBackoffInput};

use crate::runtime;

const CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING: &str = "control_plane_tick_context_failed";

struct ControlPlaneProjectTick {
	snapshot: Option<OperatorStatusSnapshot>,
	project_status: Option<OperatorProjectStatus>,
}

struct ConnectorBackoffStatusParts<'a> {
	project_id: &'a str,
	connector: &'a str,
	sync_phase: &'a str,
	quota_class: &'a str,
	reset_unix_epoch: i64,
	reset_source: &'a str,
	warning: &'a str,
}

impl TrackerConnectorBackoff {
	fn to_operator_status(
		&self,
		project_id: &str,
		now_unix_epoch: i64,
	) -> OperatorConnectorBackoffStatus {
		operator_connector_backoff_status(
			ConnectorBackoffStatusParts {
				project_id,
				connector: "linear",
				sync_phase: self.sync_phase,
				quota_class: "linear_graphql_api",
				reset_unix_epoch: self.reset_unix_epoch,
				reset_source: self.reset_source,
				warning: TRACKER_RATE_LIMIT_WARNING,
			},
			now_unix_epoch,
		)
	}
}

pub(crate) fn run_once(request: RunOnceRequest<'_>) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(request.config_path, &state_store)? else {
		if request.dry_run {
			println!("dry run: no Decodex project config supplied or registered; nothing to execute.");

			return Ok(());
		}

		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};

	runtime::register_project_config(&state_store, &config_path, true)?;

	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;

	if let Some(status) =
		active_stored_tracker_backoff_status(&state_store, config.service_id())?
	{
		print!("{}", render_tracker_backoff_cli_message("run", &status));

		return Ok(());
	}

	let preferred_run_identity = match (request.preferred_run_id, request.preferred_attempt_number)
	{
		(Some(run_id), Some(attempt_number)) =>
			Some(PreferredRunIdentity { run_id, attempt_number }),
		(None, None) => None,
		_ => eyre::bail!("preferred run identity requires both `run_id` and `attempt_number`."),
	};

	if request.explain_queue {
		return run_queue_explain(
			&config,
			&workflow,
			&state_store,
			request.dry_run,
			request.preferred_issue_id,
		);
	}

	let run_summary = match run_configured_cycle(RunCycleRequest {
		config_path: &config_path,
		state_store: &state_store,
		dry_run: request.dry_run,
		preferred_issue_id: request.preferred_issue_id,
		preferred_issue_state: request.preferred_issue_state,
		preferred_initial_issue_state: request.preferred_initial_issue_state,
		preferred_lease_acquired: request.preferred_lease_acquired,
		preferred_issue_claim_fd: request.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: request.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: request.preferred_dispatch_slot_index,
		preferred_dispatch_mode: request.preferred_dispatch_mode,
			preferred_run_identity,
			preferred_retry_budget_base: request.preferred_retry_budget_base,
			preferred_workflow_snapshot: request.preferred_workflow_snapshot,
		}) {
		Ok(summary) => summary,
		Err(error) => {
			let Some(backoff) = tracker_rate_limit_backoff(&error, Instant::now(), "run_cycle")
			else {
				return Err(error);
			};
			let status = backoff
				.to_operator_status(config.service_id(), OffsetDateTime::now_utc().unix_timestamp());

			persist_tracker_backoff_state(&state_store, config.service_id(), &backoff);

			print!("{}", render_tracker_backoff_cli_message("run", &status));

			return Ok(());
		},
	};

	if let Some(summary) = run_summary {
		clear_tracker_backoff_state_best_effort(&state_store, config.service_id());

		println!("{}", format_run_once_summary(&summary, request.dry_run));

		return Ok(());
	}

	clear_tracker_backoff_state_best_effort(&state_store, config.service_id());

	println!("{}", format_no_eligible_issue_message(&config, &workflow));

	Ok(())
}

pub(crate) fn run_control_plane(request: ServeRequest<'_>) -> Result<()> {
	if request.dev && request.config_path.is_some() {
		eyre::bail!(
			"serve --dev does not accept --config because it must not register or poll projects."
		);
	}

	validate_daemon_runtime()?;

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

			publish_operator_snapshot(&operator_state_endpoint, &snapshot);
			sleep_until_next_tick(OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL, tick_started_at);
		}
	}

	if let Some(config_path) = request.config_path {
		let Some(config_path) = resolve_config_path(Some(config_path), &state_store)? else {
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

		publish_operator_snapshot(&operator_state_endpoint, &snapshot);
		sleep_until_next_tick(DEFAULT_CONTROL_PLANE_POLL_INTERVAL, tick_started_at);
	}
}

pub(crate) fn print_status(
	config_path: Option<&Path>,
	json: bool,
	limit: usize,
	live: bool,
) -> Result<()> {
	if limit == 0 {
		eyre::bail!("`status --limit` must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let snapshot = if live {
		build_live_status_command_snapshot(&config, &workflow, &state_store, limit)?
	} else {
		build_operator_state_snapshot_without_live_observers(
			&config,
			&workflow,
			&state_store,
			limit,
		)?
	};

	print_operator_status_snapshot(&snapshot, json)?;

	Ok(())
}

pub(crate) fn run_diagnose(request: DiagnoseRequest<'_>) -> Result<()> {
	if request.limit == 0 {
		eyre::bail!("`diagnose --limit` must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(request.config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let mut snapshot = match config.tracker().resolve_api_key().and_then(LinearClient::new) {
		Ok(tracker) => build_diagnose_live_snapshot(
			&tracker,
			&config,
			&workflow,
			&state_store,
			request.limit,
		),
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = config.service_id(),
				"Skipped live diagnose observer because tracker credentials were unavailable."
			);

			let mut snapshot =
				build_operator_status_snapshot(&config, &state_store, request.limit)?;

			add_operator_snapshot_warning(
				&mut snapshot,
				"diagnose_tracker_observer_unavailable",
			);

			Ok(snapshot)
		},
	}?;

	refresh_operator_project_summary(&mut snapshot);

	let results = write_agent_evidence_snapshot(
		&snapshot,
		AgentEvidenceSource::DiagnoseCommand,
	)?;
	let result = results
		.into_iter()
		.find(|result| result.project_id == config.service_id())
		.ok_or_else(|| {
			eyre::eyre!(
				"Agent evidence writer did not produce an index for project `{}`.",
				config.service_id()
			)
		})?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&result.handoff_index)?);
	} else {
		println!("{}", render_agent_evidence_write_result(&result));
	}

	Ok(())
}

pub(crate) fn print_private_evidence(request: EvidenceRequest<'_>) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(request.config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let readback = build_private_evidence_readback(&state_store, &config, &request)?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&readback)?);
	} else {
		print!("{}", render_private_evidence_readback(&readback));
	}

	Ok(())
}

fn build_live_status_command_snapshot(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	if let Some(status) = active_stored_tracker_backoff_status(state_store, config.service_id())? {
		return build_operator_status_snapshot_for_tracker_backoff(
			config,
			state_store,
			limit,
			&status,
		);
	}

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let recovered_state =
		recover_runtime_state_from_tracker_and_worktrees(&tracker, config, workflow, state_store);
	let mut snapshot_warnings = Vec::new();

	match recovered_state {
		Ok(recovered_state) =>
			hydrate_status_snapshot_state(config, state_store, recovered_state)?,
		Err(error) => {
			if let Some(backoff) =
				tracker_rate_limit_backoff(&error, Instant::now(), "runtime_recovery")
			{
				let status = backoff.to_operator_status(
					config.service_id(),
					OffsetDateTime::now_utc().unix_timestamp(),
				);

				persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

				return build_operator_status_snapshot_for_tracker_backoff(
					config,
					state_store,
					limit,
					&status,
				);
			}

			let warning = runtime_recovery_warning("runtime_recovery_unavailable", &error);

			tracing::warn!(
				recovery_error_class = runtime_recovery_error_class(&error),
				"Skipped runtime recovery for operator status; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(warning);
		},
	}

	let mut snapshot = match build_status_command_operator_status_snapshot(
		&tracker,
		config,
		workflow,
		state_store,
		limit,
	) {
		Ok(snapshot) => snapshot,
		Err(error) => {
			let Some(backoff) =
				tracker_rate_limit_backoff(&error, Instant::now(), "operator_status_refresh")
			else {
				return Err(error);
			};
			let status = backoff.to_operator_status(
				config.service_id(),
				OffsetDateTime::now_utc().unix_timestamp(),
			);

			persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

			build_operator_status_snapshot_for_tracker_backoff(config, state_store, limit, &status)?
		},
	};

	for warning in snapshot_warnings {
		add_operator_snapshot_warning(&mut snapshot, &warning);
	}

	refresh_operator_project_summary(&mut snapshot);

	if !snapshot
		.connector_backoffs
		.iter()
		.any(|backoff| backoff.connector == "linear")
	{
		clear_tracker_backoff_state_best_effort(state_store, config.service_id());
	}

	Ok(snapshot)
}

fn run_queue_explain(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	preferred_issue_id: Option<&str>,
) -> Result<()> {
	if !dry_run {
		eyre::bail!("queue explanation is only supported for dry-run execution.");
	}
	if preferred_issue_id.is_some() {
		eyre::bail!("queue explanation does not accept a preferred issue.");
	}

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let queued_candidates =
		match build_queued_candidate_statuses(&tracker, config, workflow, state_store) {
			Ok(queued_candidates) => queued_candidates,
			Err(error) => {
				let Some(backoff) =
					tracker_rate_limit_backoff(&error, Instant::now(), "queue_explain")
				else {
					return Err(error);
				};
				let status =
					backoff.to_operator_status(config.service_id(), OffsetDateTime::now_utc().unix_timestamp());

				persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

				print!("{}", render_tracker_backoff_cli_message("run", &status));

				return Ok(());
			},
		};

	print!("{}", render_queue_explain(config, &queued_candidates));

	Ok(())
}

fn print_operator_status_snapshot(
	snapshot: &OperatorStatusSnapshot,
	json: bool,
) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string_pretty(snapshot)?);
	} else {
		print!("{}", render_operator_status(snapshot));
	}

	Ok(())
}

fn operator_connector_backoff_status(
	parts: ConnectorBackoffStatusParts<'_>,
	now_unix_epoch: i64,
) -> OperatorConnectorBackoffStatus {
	OperatorConnectorBackoffStatus {
		project_id: parts.project_id.to_owned(),
		connector: parts.connector.to_owned(),
		sync_phase: parts.sync_phase.to_owned(),
		quota_class: parts.quota_class.to_owned(),
		reset_at: format_optional_unix_timestamp(Some(parts.reset_unix_epoch))
			.unwrap_or_else(|| parts.reset_unix_epoch.to_string()),
		reset_unix_epoch: parts.reset_unix_epoch,
		reset_source: parts.reset_source.to_owned(),
		retry_after_seconds: parts.reset_unix_epoch.saturating_sub(now_unix_epoch).max(0),
		next_action: String::from(
			"Wait for the reset window; keep monitoring local running lanes.",
		),
		warning: parts.warning.to_owned(),
	}
}

fn connector_backoff_record_to_operator_status(
	backoff: &ConnectorBackoff,
	now_unix_epoch: i64,
) -> OperatorConnectorBackoffStatus {
	operator_connector_backoff_status(
		ConnectorBackoffStatusParts {
			project_id: backoff.project_id(),
			connector: backoff.connector(),
			sync_phase: backoff.sync_phase(),
			quota_class: backoff.quota_class(),
			reset_unix_epoch: backoff.reset_unix_epoch(),
			reset_source: backoff.reset_source(),
			warning: backoff.warning(),
		},
		now_unix_epoch,
	)
}

fn active_stored_tracker_backoff_status(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Option<OperatorConnectorBackoffStatus>> {
	let Some(backoff) = state_store.connector_backoff(project_id, "linear")? else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		state_store.clear_connector_backoff(project_id, "linear")?;

		return Ok(None);
	}

	Ok(Some(connector_backoff_record_to_operator_status(&backoff, now_unix_epoch)))
}

fn active_stored_tracker_backoff_status_best_effort(
	state_store: &StateStore,
	project_id: &str,
) -> Option<OperatorConnectorBackoffStatus> {
	match active_stored_tracker_backoff_status(state_store, project_id) {
		Ok(status) => status,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project_id,
				"Failed to read persisted tracker backoff; sensitive runtime details were withheld."
			);

			None
		},
	}
}

fn persist_tracker_backoff_state(
	state_store: &StateStore,
	project_id: &str,
	backoff: &TrackerConnectorBackoff,
) {
	if let Err(error) = state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id,
		connector: "linear",
		sync_phase: backoff.sync_phase,
		quota_class: "linear_graphql_api",
		reset_unix_epoch: backoff.reset_unix_epoch,
		reset_source: backoff.reset_source,
		warning: TRACKER_RATE_LIMIT_WARNING,
	}) {
		let _ = error;

		tracing::warn!(
			project_id = project_id,
			"Failed to persist tracker backoff; sensitive runtime details were withheld."
		);
	}
}

fn clear_tracker_backoff_state_best_effort(state_store: &StateStore, project_id: &str) {
	if let Err(error) = state_store.clear_connector_backoff(project_id, "linear") {
		let _ = error;

		tracing::warn!(
			project_id = project_id,
			"Failed to clear persisted tracker backoff; sensitive runtime details were withheld."
		);
	}
}

fn render_tracker_backoff_cli_message(
	command: &str,
	status: &OperatorConnectorBackoffStatus,
) -> String {
	format!(
		"Linear connector is in backoff for project `{}`; `{}` skipped tracker reads for `{}` until {} (retry_after_seconds={}).\n",
		status.project_id,
		command,
		status.sync_phase,
		status.reset_at,
		status.retry_after_seconds
	)
}

fn build_operator_status_snapshot_for_tracker_backoff(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
	status: &OperatorConnectorBackoffStatus,
) -> Result<OperatorStatusSnapshot> {
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let mut snapshot = build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;

	snapshot.post_review_lanes =
		build_degraded_post_review_lane_statuses(project, state_store, &review_state_inspector)?;

	add_operator_snapshot_warning(&mut snapshot, TRACKER_RATE_LIMIT_WARNING);

	snapshot.connector_backoffs.push(status.clone());

	add_operator_snapshot_warning(&mut snapshot, "external_observer_status_skipped");
	apply_terminal_history_ledger_outcomes(&mut snapshot);
	refresh_operator_project_summary(&mut snapshot);

	Ok(snapshot)
}

fn publish_operator_snapshot(
	operator_state_endpoint: &OperatorStateEndpoint,
	snapshot: &OperatorStatusSnapshot,
) {
	if let Err(error) = operator_state_endpoint.publish_snapshot(snapshot) {
		let _ = error;

		tracing::warn!(
			"Operator snapshot publish failed; sensitive runtime details were withheld from control-plane logs."
		);
	}
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

fn runtime_recovery_warning(prefix: &str, error: &Report) -> String {
	format!("{prefix}:{}", runtime_recovery_error_class(error))
}

fn runtime_recovery_error_class(error: &Report) -> &'static str {
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

fn build_diagnose_live_snapshot<T>(
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

	match recover_runtime_state_from_tracker_and_worktrees(tracker, config, workflow, state_store) {
		Ok(recovered_state) =>
			hydrate_status_snapshot_state(config, state_store, recovered_state)?,
		Err(error) => {
			let warning =
				runtime_recovery_warning("diagnose_runtime_recovery_unavailable", &error);

			tracing::warn!(
				project_id = config.service_id(),
				recovery_error_class = runtime_recovery_error_class(&error),
				"Skipped runtime recovery for diagnose; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(warning);
		},
	}

	let mut snapshot = match build_live_operator_status_snapshot(
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

			build_operator_status_snapshot(config, state_store, limit)?
		},
	};

	for warning in snapshot_warnings {
		add_operator_snapshot_warning(&mut snapshot, &warning);
	}

	Ok(snapshot)
}

#[cfg(test)]
fn run_control_plane_tick(
	state_store: &StateStore,
	project_runtimes: &mut HashMap<String, ProjectDaemonRuntime>,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> Result<OperatorStatusSnapshot> {
	run_control_plane_tick_with_options(state_store, project_runtimes, linear_scan_requests)
}

fn run_control_plane_tick_with_options(
	state_store: &StateStore,
	project_runtimes: &mut HashMap<String, ProjectDaemonRuntime>,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> Result<OperatorStatusSnapshot> {
	let registered_projects = state_store.list_projects()?;
	let now = Instant::now();

	Ok(collect_control_plane_snapshot(registered_projects, |project, project_warnings| {
		if project.enabled() {
			let runtime = project_runtimes.entry(project.service_id().to_owned()).or_default();

			run_control_plane_project_tick(
				project,
				state_store,
				runtime,
				project_warnings,
				linear_scan_requests,
				now,
			)
		} else {
			control_plane_disabled_project_observer_tick(project, state_store, project_warnings)
		}
	}))
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

fn run_control_plane_dev_tick(state_store: &StateStore) -> Result<OperatorStatusSnapshot> {
	let registered_projects = state_store.list_projects()?;
	let mut snapshot = empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);
	let mut project_statuses = Vec::new();

	if !registered_projects.iter().any(ProjectRegistration::enabled) {
		add_operator_snapshot_warning(&mut snapshot, "no_enabled_projects");
	}

	add_operator_snapshot_warning(&mut snapshot, "automation_disabled");

	for registration in &registered_projects {
		let mut project_status = operator_project_status_from_dev_registration(registration);

		if registration.enabled() {
			match ServiceConfig::from_path(registration.config_path()).and_then(|project| {
				build_operator_status_snapshot(
					&project,
					state_store,
					DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
				)
			}) {
				Ok(project_snapshot) => {
					hydrate_project_status_from_local_snapshot(
						&mut project_status,
						&project_snapshot,
					);
					append_control_plane_project_snapshot(&mut snapshot, project_snapshot);
				},
				Err(error) => {
					let _ = error;

					project_status.connector_state = String::from("config_error");
					project_status.warning_count = project_status.warning_count.saturating_add(1);

					add_operator_snapshot_warning(&mut snapshot, "operator_snapshot_build_failed");

					tracing::warn!(
						project_id = registration.service_id(),
						"Dev operator snapshot local run hydration failed; sensitive runtime details were withheld."
					);
				},
			}
		} else {
			let mut project_warnings = Vec::new();
			let project_tick = control_plane_disabled_project_observer_tick(
				registration,
				state_store,
				&mut project_warnings,
			);

			for warning in project_warnings {
				add_operator_snapshot_warning(&mut snapshot, warning);
			}

			if let Some(local_status) = project_tick.project_status {
				project_status = operator_project_status_from_dev_registration(registration);

				hydrate_project_status_from_registered_status(
					&mut project_status,
					&local_status,
				);
			}
			if let Some(project_snapshot) = project_tick.snapshot {
				append_control_plane_project_snapshot(&mut snapshot, project_snapshot);
			}
		}

		project_statuses.push(project_status);
	}

	snapshot.projects = project_statuses;
	snapshot.account_control = global_codex_account_control_status();

	Ok(snapshot)
}

fn collect_control_plane_snapshot<F>(
	registered_projects: Vec<ProjectRegistration>,
	mut run_project_tick: F,
) -> OperatorStatusSnapshot
where
	F: FnMut(&ProjectRegistration, &mut Vec<&'static str>) -> ControlPlaneProjectTick,
{
	let registered_project_count = registered_projects.len();
	let mut snapshot_warnings = Vec::new();
	let mut project_statuses = Vec::new();
	let mut project_snapshots = Vec::new();

	if !registered_projects.iter().any(ProjectRegistration::enabled) {
		snapshot_warnings.push("no_enabled_projects");
	}

	for project in registered_projects {
		let mut project_warnings = Vec::new();
		let project_tick = run_project_tick(&project, &mut project_warnings);

		snapshot_warnings.extend(project_warnings);

		if let Some(status) = project_tick.project_status {
			project_statuses.push(status);
		}
		if let Some(snapshot) = project_tick.snapshot {
			project_snapshots.push(snapshot);
		}
	}

	let mut snapshot =
		aggregate_control_plane_snapshot(registered_project_count, project_snapshots);

	snapshot.projects = project_statuses;
	snapshot.account_control = global_codex_account_control_status();

	for warning in snapshot_warnings {
		add_operator_snapshot_warning(&mut snapshot, warning);
	}

	snapshot
}

fn control_plane_disabled_project_observer_tick(
	project: &ProjectRegistration,
	state_store: &StateStore,
	snapshot_warnings: &mut Vec<&'static str>,
) -> ControlPlaneProjectTick {
	let project_status = operator_project_status_from_registration(project, 0);
	let active_runs = match state_store.list_active_runs(project.service_id()) {
		Ok(active_runs) => active_runs,
		Err(error) => {
			let _ = error;

			tracing::warn!(
					project_id = project.service_id(),
					"Disabled project active-run lookup failed; sensitive runtime details were withheld."
				);

				snapshot_warnings.push("operator_snapshot_build_failed");

				return ControlPlaneProjectTick {
				snapshot: None,
				project_status: Some(project_status),
			};
		},
	};

	if active_runs.is_empty() {
		return ControlPlaneProjectTick {
			snapshot: None,
			project_status: Some(project_status),
		};
	}

	match build_registered_project_local_snapshot(project, state_store) {
		Ok(project_snapshot) => {
			let mut project_status = project_status;

			hydrate_project_status_from_local_snapshot(&mut project_status, &project_snapshot);

			ControlPlaneProjectTick {
				snapshot: Some(project_snapshot),
				project_status: Some(project_status),
			}
		},
		Err(error) => {
			let _ = error;

			tracing::warn!(
					project_id = project.service_id(),
					"Disabled project active-run snapshot build failed; sensitive runtime details were withheld."
				);

				snapshot_warnings.push("operator_snapshot_build_failed");

				ControlPlaneProjectTick {
				snapshot: None,
				project_status: Some(project_status),
			}
		},
	}
}

fn build_registered_project_local_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
) -> Result<OperatorStatusSnapshot> {
	let config = ServiceConfig::from_path(project.config_path())?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	build_operator_state_snapshot_without_live_observers(
		&config,
		&workflow,
		state_store,
		DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
	)
}

fn hydrate_project_status_from_local_snapshot(
	project_status: &mut OperatorProjectStatus,
	project_snapshot: &OperatorStatusSnapshot,
) {
	if let Some(local_status) = project_snapshot.projects.first() {
		hydrate_project_status_from_registered_status(project_status, local_status);
	} else {
		project_status.active_run_count = project_snapshot.active_runs.len();
		project_status.running_lane_count = project_snapshot
			.active_runs
			.iter()
			.filter(|run| operator_run_counts_as_running(run))
			.count();
	}
}

fn hydrate_project_status_from_registered_status(
	project_status: &mut OperatorProjectStatus,
	local_status: &OperatorProjectStatus,
) {
	project_status.active_run_count = local_status.active_run_count;
	project_status.running_lane_count = local_status.running_lane_count;
	project_status.retained_worktree_count = local_status.retained_worktree_count;
	project_status.waiting_lane_count = local_status.waiting_lane_count;
	project_status.attention_count = local_status.attention_count;
	project_status.cleanup_blocked_count = local_status.cleanup_blocked_count;
	project_status.cleanup_pending_count = local_status.cleanup_pending_count;
	project_status.last_activity_at = local_status.last_activity_at.clone();
	project_status.warning_count =
		project_status.warning_count.saturating_add(local_status.warning_count);
}

fn aggregate_control_plane_snapshot(
	registered_project_count: usize,
	mut project_snapshots: Vec<OperatorStatusSnapshot>,
) -> OperatorStatusSnapshot {
	if registered_project_count == 1 && project_snapshots.len() == 1 {
		return project_snapshots.remove(0);
	}

	let mut snapshot = empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);

	for project_snapshot in project_snapshots {
		append_control_plane_project_snapshot(&mut snapshot, project_snapshot);
	}

	snapshot
}

fn append_control_plane_project_snapshot(
	snapshot: &mut OperatorStatusSnapshot,
	project_snapshot: OperatorStatusSnapshot,
) {
	for warning in project_snapshot.warnings {
		add_operator_snapshot_warning(snapshot, &warning);
	}

	snapshot.warning_details.extend(project_snapshot.warning_details);
	snapshot.connector_backoffs.extend(project_snapshot.connector_backoffs);
	snapshot.accounts.extend(project_snapshot.accounts);
	snapshot.active_runs.extend(project_snapshot.active_runs);
	snapshot.recent_runs.extend(project_snapshot.recent_runs);
	snapshot.history_lanes.extend(project_snapshot.history_lanes);
	snapshot.queued_candidates.extend(project_snapshot.queued_candidates);
	snapshot.worktrees.extend(project_snapshot.worktrees);
	snapshot.post_review_lanes.extend(project_snapshot.post_review_lanes);
}

fn run_control_plane_project_tick(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	snapshot_warnings: &mut Vec<&'static str>,
	linear_scan_requests: &[OperatorLinearScanRequest],
	now: Instant,
) -> ControlPlaneProjectTick {
	if tracker_backoff_active(runtime, now) {
		snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

		let connector_backoffs = active_connector_backoff_statuses(project.service_id(), runtime);

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
		snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

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
		Ok(context) =>
			control_plane_project_snapshot(
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

fn linear_scan_due(
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

fn remember_next_linear_scan(runtime: &mut ProjectDaemonRuntime, now: Instant) {
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
	let backoff = tracker_rate_limit_backoff(error, now, sync_phase)?;
	let status =
		backoff.to_operator_status(project_id, OffsetDateTime::now_utc().unix_timestamp());

	persist_tracker_backoff_state(state_store, project_id, &backoff);

	runtime.tracker_backoff = Some(backoff);

	Some(status)
}

fn tracker_rate_limit_backoff(
	error: &Report,
	now: Instant,
	sync_phase: &'static str,
) -> Option<TrackerConnectorBackoff> {
	let message = format!("{error:#}");

	if !message.contains("Linear connector is rate limited") {
		return None;
	}

	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let fallback_reset_unix_epoch =
		now_unix_epoch.saturating_add(TRACKER_RATE_LIMIT_BACKOFF_SECS as i64);
	let (reset_unix_epoch, reset_source) =
		match parse_linear_rate_limit_reset_unix_epoch(&message) {
			Some(reset_unix_epoch) if reset_unix_epoch > now_unix_epoch =>
				(reset_unix_epoch, "linear"),
			_ => (fallback_reset_unix_epoch, "local_default"),
		};
	let retry_after_seconds = reset_unix_epoch - now_unix_epoch;
	let retry_after_seconds = u64::try_from(retry_after_seconds).ok()?;

	Some(TrackerConnectorBackoff {
		until: now + Duration::from_secs(retry_after_seconds),
		reset_unix_epoch,
		reset_source,
		sync_phase,
	})
}

fn active_connector_backoff_statuses(
	project_id: &str,
	runtime: &ProjectDaemonRuntime,
) -> Vec<OperatorConnectorBackoffStatus> {
	let Some(backoff) = runtime.tracker_backoff.as_ref() else {
		return Vec::new();
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	vec![backoff.to_operator_status(project_id, now_unix_epoch)]
}

fn parse_linear_rate_limit_reset_unix_epoch(message: &str) -> Option<i64> {
	let reset = message.split("rate limited until `").nth(1)?.split('`').next()?;

	reset.parse().ok()
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
				write_agent_evidence_best_effort(&snapshot, AgentEvidenceSource::ServeTick);

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

fn build_operator_state_snapshot_without_live_observers(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	state_store.configure_dispatch_slot_root(
		project.service_id(),
		project.worktree_root(),
		workflow.frontmatter().execution().max_concurrent_agents(),
	)?;

	let mut snapshot = build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;
	apply_terminal_history_ledger_outcomes(&mut snapshot);
	refresh_operator_project_summary(&mut snapshot);

	Ok(snapshot)
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
				write_agent_evidence_best_effort(&snapshot, AgentEvidenceSource::ServeTick);

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
			snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

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

			write_agent_evidence_best_effort(&snapshot, AgentEvidenceSource::ServeTick);

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
				snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

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

fn complete_project_status(
	project: &ProjectRegistration,
	mut status: OperatorProjectStatus,
) -> OperatorProjectStatus {
	status.config_path = project.config_path().display().to_string();
	status.enabled = project.enabled();

	status
}

fn control_plane_tick_context_failed_tick(
	project: &ProjectRegistration,
	error: &Report,
	warning_count: usize,
) -> ControlPlaneProjectTick {
	let mut snapshot = empty_control_plane_snapshot(DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT);

	add_operator_snapshot_warning(&mut snapshot, CONTROL_PLANE_TICK_CONTEXT_FAILED_WARNING);

	snapshot
		.warning_details
		.push(control_plane_tick_context_failed_warning_detail(project, error));

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

fn empty_control_plane_snapshot(limit: usize) -> OperatorStatusSnapshot {
	OperatorStatusSnapshot {
		project_id: String::from("all"),
		run_limit: limit,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		queued_candidates: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	}
}

fn operator_project_status_from_registration(
	project: &ProjectRegistration,
	warning_count: usize,
) -> OperatorProjectStatus {
	OperatorProjectStatus {
		project_id: project.service_id().to_owned(),
		config_path: project.config_path().display().to_string(),
		repo_root: project.repo_root().display().to_string(),
		enabled: project.enabled(),
		github_cli_authority: operator_github_cli_authority_from_registration(project),
		active_run_count: 0,
		running_lane_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
		cleanup_blocked_count: 0,
		cleanup_pending_count: 0,
		connector_state: if project.enabled() {
			if warning_count == 0 {
				String::from("ok")
			} else {
				String::from("degraded")
			}
		} else {
			String::from("disabled")
		},
		last_activity_at: None,
		warning_count,
	}
}

fn operator_project_status_from_dev_registration(
	project: &ProjectRegistration,
) -> OperatorProjectStatus {
	OperatorProjectStatus {
		project_id: project.service_id().to_owned(),
		config_path: project.config_path().display().to_string(),
		repo_root: project.repo_root().display().to_string(),
		enabled: project.enabled(),
		github_cli_authority: operator_github_cli_authority_from_registration(project),
		active_run_count: 0,
		running_lane_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
		cleanup_blocked_count: 0,
		cleanup_pending_count: 0,
		connector_state: if project.enabled() {
			String::from("dev")
		} else {
			String::from("disabled")
		},
		last_activity_at: None,
		warning_count: usize::from(project.enabled()),
	}
}
