use crate::runtime;

struct ControlPlaneProjectTick {
	snapshot: Option<OperatorStatusSnapshot>,
	project_status: Option<OperatorProjectStatus>,
}

impl TrackerConnectorBackoff {
	fn to_operator_status(
		&self,
		project_id: &str,
		now_unix_epoch: i64,
	) -> OperatorConnectorBackoffStatus {
		OperatorConnectorBackoffStatus {
			project_id: project_id.to_owned(),
			connector: String::from("linear"),
			sync_phase: self.sync_phase.to_owned(),
			quota_class: String::from("linear_graphql_api"),
			reset_at: format_optional_unix_timestamp(Some(self.reset_unix_epoch))
				.unwrap_or_else(|| self.reset_unix_epoch.to_string()),
			reset_unix_epoch: self.reset_unix_epoch,
			reset_source: self.reset_source.to_owned(),
			retry_after_seconds: self.reset_unix_epoch.saturating_sub(now_unix_epoch).max(0),
			next_action: String::from(
				"Wait for the reset window; keep monitoring local running lanes.",
			),
			warning: String::from(TRACKER_RATE_LIMIT_WARNING),
		}
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
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};

	runtime::register_project_config(&state_store, &config_path, true)?;

	let preferred_run_identity = match (request.preferred_run_id, request.preferred_attempt_number)
	{
		(Some(run_id), Some(attempt_number)) =>
			Some(PreferredRunIdentity { run_id, attempt_number }),
		(None, None) => None,
		_ => eyre::bail!("preferred run identity requires both `run_id` and `attempt_number`."),
	};

	if let Some(summary) = run_configured_cycle(RunCycleRequest {
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
	})? {
		println!("{}", format_run_once_summary(&summary, request.dry_run));

		return Ok(());
	}

	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;

	println!("{}", format_no_eligible_issue_message(&config, &workflow));

	Ok(())
}

pub(crate) fn run_control_plane(request: ServeRequest<'_>) -> Result<()> {
	if request.poll_interval.is_zero() {
		eyre::bail!("serve interval must be greater than zero.");
	}

	validate_daemon_runtime()?;

	let state_store = Arc::new(runtime::open_runtime_store()?);

	if let Some(config_path) = request.config_path {
		let Some(config_path) = resolve_config_path(Some(config_path), &state_store)? else {
			eyre::bail!(
				"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
			);
		};

		runtime::register_project_config(&state_store, &config_path, true)?;
	}

	let operator_state_endpoint = OperatorStateEndpoint::start(
		request.listen_address,
		operator_snapshot_ready_stale_after(request.poll_interval),
		Arc::clone(&state_store),
	)?;
	let runtime_db_path = runtime::runtime_db_path()?;
	let global_config_path = runtime::global_config_path()?;
	let project_config_dir = runtime::project_config_dir()?;
	let mut project_runtimes: HashMap<String, ProjectDaemonRuntime> = HashMap::new();

	tracing::info!(
		poll_interval_s = request.poll_interval.as_secs(),
		listen_address = %operator_state_endpoint.listen_address(),
		path = OPERATOR_STATE_ENDPOINT_PATH,
		runtime_db_path = %runtime_db_path.display(),
		global_config_path = %global_config_path.display(),
		project_config_dir = %project_config_dir.display(),
		"Starting Decodex control-plane poll loop."
	);

	loop {
		let tick_started_at = Instant::now();
		let snapshot = run_control_plane_tick(&state_store, &mut project_runtimes)?;

		if let Err(error) = operator_state_endpoint.publish_snapshot(&snapshot) {
			let _ = error;

			tracing::warn!(
				"Operator snapshot publish failed; sensitive runtime details were withheld from control-plane logs."
			);
		}

		sleep_until_next_tick(request.poll_interval, tick_started_at);
	}
}

pub(crate) fn print_status(
	config_path: Option<&Path>,
	json: bool,
	limit: usize,
) -> Result<()> {
	if limit == 0 {
		eyre::bail!("`status --limit` must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let recovered_state = recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	);
	let mut snapshot_warnings = Vec::new();

	match recovered_state {
		Ok(recovered_state) =>
			hydrate_status_snapshot_state(&config, &state_store, recovered_state)?,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				"Skipped runtime recovery for operator status; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("runtime_recovery_unavailable");
		},
	}

	let mut snapshot =
		build_live_operator_status_snapshot(&tracker, &config, &workflow, &state_store, limit)?;

	for warning in snapshot_warnings {
		add_operator_snapshot_warning(&mut snapshot, warning);
	}

	refresh_operator_project_summary(&mut snapshot);

	if json {
		println!("{}", serde_json::to_string_pretty(&snapshot)?);
	} else {
		print!("{}", render_operator_status(&snapshot));
	}

	Ok(())
}

pub(crate) fn run_diagnose(request: DiagnoseRequest<'_>) -> Result<()> {
	if request.limit == 0 {
		eyre::bail!("`diagnose --limit` must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(request.config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
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
			let _ = error;

			tracing::warn!(
				project_id = config.service_id(),
				"Skipped runtime recovery for diagnose; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("diagnose_runtime_recovery_unavailable");
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

			snapshot_warnings.push("diagnose_live_observer_unavailable");

			build_operator_status_snapshot(config, state_store, limit)?
		},
	};

	for warning in snapshot_warnings {
		add_operator_snapshot_warning(&mut snapshot, warning);
	}

	Ok(snapshot)
}

fn run_control_plane_tick(
	state_store: &StateStore,
	project_runtimes: &mut HashMap<String, ProjectDaemonRuntime>,
) -> Result<OperatorStatusSnapshot> {
	let registered_projects = state_store.list_projects()?;

	Ok(collect_control_plane_snapshot(registered_projects, |project, project_warnings| {
		let runtime = project_runtimes.entry(project.service_id().to_owned()).or_default();

		run_control_plane_project_tick(project, state_store, runtime, project_warnings)
	}))
}

fn collect_control_plane_snapshot<F>(
	registered_projects: Vec<ProjectRegistration>,
	mut run_enabled_project_tick: F,
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
		if !project.enabled() {
			project_statuses.push(operator_project_status_from_registration(&project, 0));

			continue;
		}

		let mut project_warnings = Vec::new();
		let project_tick = run_enabled_project_tick(&project, &mut project_warnings);

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
) -> ControlPlaneProjectTick {
	if tracker_backoff_active(runtime, Instant::now()) {
		snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

		return control_plane_project_local_snapshot(project, state_store, runtime, snapshot_warnings);
	}

	match load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache) {
		Ok(context) =>
			control_plane_project_snapshot(project, state_store, runtime, &context, snapshot_warnings),
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Control-plane tick context failed; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("control_plane_tick_context_failed");

			ControlPlaneProjectTick {
				snapshot: None,
				project_status: Some(operator_project_status_from_registration(project, 1)),
			}
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

fn remember_tracker_backoff(
	runtime: &mut ProjectDaemonRuntime,
	error: &Report,
	now: Instant,
	sync_phase: &'static str,
) -> bool {
	let Some(backoff) = tracker_rate_limit_backoff(error, now, sync_phase) else {
		return false;
	};

	runtime.tracker_backoff = Some(backoff);

	true
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

fn control_plane_project_local_snapshot(
	project: &ProjectRegistration,
	state_store: &StateStore,
	runtime: &mut ProjectDaemonRuntime,
	snapshot_warnings: &mut Vec<&'static str>,
) -> ControlPlaneProjectTick {
	match load_daemon_tick_context(project.config_path(), &mut runtime.workflow_cache) {
		Ok(context) => match build_operator_state_snapshot_for_publish(
			&context.tracker,
			&context.config,
			&context.workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
			snapshot_warnings,
			&active_connector_backoff_statuses(project.service_id(), runtime),
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
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Control-plane local snapshot context failed; sensitive runtime details were withheld."
			);

			snapshot_warnings.push("control_plane_tick_context_failed");
			ControlPlaneProjectTick {
				snapshot: None,
				project_status: Some(operator_project_status_from_registration(
					project,
					snapshot_warnings.len(),
				)),
			}
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
		context,
	) {
		if remember_tracker_backoff(runtime, &error, Instant::now(), "control_plane_tick") {
			snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

			return control_plane_project_local_snapshot(
				project,
				state_store,
				runtime,
				snapshot_warnings,
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
			runtime.tracker_backoff = None;

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
			if remember_tracker_backoff(runtime, &error, Instant::now(), "operator_snapshot_refresh") {
				snapshot_warnings.push(TRACKER_RATE_LIMIT_WARNING);

				return control_plane_project_local_snapshot(
					project,
					state_store,
					runtime,
					snapshot_warnings,
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

fn complete_project_status(
	project: &ProjectRegistration,
	mut status: OperatorProjectStatus,
) -> OperatorProjectStatus {
	status.config_path = project.config_path().display().to_string();
	status.enabled = project.enabled();

	status
}

fn empty_control_plane_snapshot(limit: usize) -> OperatorStatusSnapshot {
	OperatorStatusSnapshot {
		project_id: String::from("all"),
		run_limit: limit,
		warnings: Vec::new(),
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
		active_run_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
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
