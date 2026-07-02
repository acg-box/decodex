use std::{
	io::{self, ErrorKind, Write},
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

use serde_json::{self, Value};

use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, AccountActivityMode, AgentEvidenceSource, DiagnoseRequest, EvidenceRequest,
		LaneSteerRequest, OperatorStateEndpoint, OperatorStatusSnapshot, PreferredRunIdentity,
		RunCycleRequest, RunOnceRequest, build_operator_status_snapshot_with_account_mode,
		build_private_evidence_readback, build_queued_candidate_statuses,
		build_status_command_operator_status_snapshot, clear_tracker_backoff_state_best_effort,
		format_no_eligible_issue_message, format_run_once_summary, hydrate_status_snapshot_state,
		lane_control, load_configured_cycle_workflow, persist_tracker_backoff_state,
		recover_runtime_state_from_tracker_and_worktrees, refresh_operator_project_summary,
		render_agent_evidence_write_result, render_operator_status,
		render_private_evidence_readback, render_queue_explain, render_tracker_backoff_cli_message,
		resolve_config_path, run_configured_cycle, runtime_recovery_error_class,
		runtime_recovery_warning, status_should_attempt_operator_snapshot_cache,
		status_snapshot_from_local_operator_cache, tracker_connector_backoff,
		write_agent_evidence_snapshot,
	},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(crate) struct McpLaneSteerRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) expected_turn_id: &'a str,
	pub(crate) message: &'a str,
	pub(crate) source: &'a str,
	pub(crate) wait_timeout: Duration,
}

pub(crate) fn run_once(request: RunOnceRequest<'_>) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(request.config_path, &state_store)? else {
		if request.dry_run {
			println!(
				"dry run: no Decodex project config supplied or registered; nothing to execute."
			);

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
		orchestrator::active_stored_tracker_backoff_status(&state_store, config.service_id())?
	{
		print!("{}", render_tracker_backoff_cli_message("run", &status));

		return Ok(());
	}

	let preferred_run_identity = match (request.preferred_run_id, request.preferred_attempt_number)
	{
		(Some(run_id), Some(attempt_number)) => {
			Some(PreferredRunIdentity { run_id, attempt_number })
		},
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
			let Some(backoff) = tracker_connector_backoff(&error, Instant::now(), "run_cycle")
			else {
				return Err(error);
			};
			let status = backoff.to_operator_status(
				config.service_id(),
				OffsetDateTime::now_utc().unix_timestamp(),
			);

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

pub(crate) fn print_status(
	config_path: Option<&Path>,
	json: bool,
	limit: usize,
	live: bool,
) -> Result<()> {
	if limit == 0 {
		eyre::bail!("`status --limit` must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let snapshot = if live {
		let mut snapshot =
			build_live_status_command_snapshot(&config, &workflow, &state_store, limit)?;

		snapshot.status_source = Some(String::from("live_observers"));

		snapshot
	} else if status_should_attempt_operator_snapshot_cache(live) {
		match status_snapshot_from_local_operator_cache(&config, limit) {
			Ok(snapshot) => snapshot,
			Err(cache_miss) => {
				let mut snapshot =
					orchestrator::build_operator_state_snapshot_without_live_observers(
						&config,
						&workflow,
						&state_store,
						limit,
					)?;

				snapshot.status_source = Some(String::from("local_runtime"));

				orchestrator::add_status_snapshot_cache_miss_warning(
					&mut snapshot,
					&config,
					cache_miss,
				);

				snapshot
			},
		}
	} else {
		let mut snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
			&config,
			&workflow,
			&state_store,
			limit,
		)?;

		snapshot.status_source = Some(String::from("local_runtime"));

		snapshot
	};

	print_operator_status_snapshot(&snapshot, json)?;

	Ok(())
}

pub(crate) fn build_mcp_status_resource(config_path: Option<&Path>, limit: usize) -> Result<Value> {
	if limit == 0 {
		eyre::bail!("MCP status resource limit must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let mut snapshot = build_operator_status_snapshot_with_account_mode(
		&config,
		&state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	snapshot.status_source = Some(String::from("local_runtime"));

	serde_json::to_value(snapshot).map_err(Into::into)
}

pub(crate) fn build_mcp_lane_control_resource(
	config_path: Option<&Path>,
	issue: Option<&str>,
	run_id: Option<&str>,
	limit: usize,
) -> Result<Value> {
	if limit == 0 {
		eyre::bail!("MCP lane-control resource limit must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;

	if let Some(issue) = issue {
		let report = lane_control::build_lane_inspect_report(&state_store, &config, issue, run_id)?;

		return serde_json::to_value(report).map_err(Into::into);
	}

	let snapshot = build_operator_status_snapshot_with_account_mode(
		&config,
		&state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	Ok(serde_json::json!({
		"schema": "decodex.mcp.lane_control_readback/1",
		"project_id": snapshot.project_id,
		"read_only": true,
		"mutating_tools": [],
		"current_lanes": snapshot.current_lanes,
		"recent_runs": snapshot.recent_runs,
		"post_review_lanes": snapshot.post_review_lanes
	}))
}

pub(crate) fn run_mcp_lane_interrupt(
	config_path: Option<&Path>,
	issue: &str,
	run_id: &str,
	force: bool,
	reason: Option<&str>,
	source: &str,
) -> Result<Value> {
	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let report = lane_control::interrupt_lane_with_state(
		&state_store,
		&config,
		issue,
		run_id,
		force,
		reason,
		source,
	)?;

	serde_json::to_value(report).map_err(Into::into)
}

pub(crate) fn run_mcp_lane_steer(request: McpLaneSteerRequest<'_>) -> Result<Value> {
	let state_store = runtime::open_runtime_store_lazy()?;
	let Some(config_path) = resolve_config_path(request.config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Start MCP from a registered checkout or pass --config."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let lane_request = LaneSteerRequest {
		config_path: Some(&config_path),
		project_id: request.project_id,
		issue: request.issue,
		run_id: request.run_id,
		expected_turn_id: request.expected_turn_id,
		message: request.message,
		source: request.source,
		wait_timeout: request.wait_timeout,
	};
	let report = lane_control::steer_lane_with_state(&state_store, &config, &lane_request)?;

	serde_json::to_value(report).map_err(Into::into)
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
		Ok(tracker) => orchestrator::build_diagnose_live_snapshot(
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
				orchestrator::build_operator_status_snapshot(&config, &state_store, request.limit)?;

			orchestrator::add_operator_snapshot_warning(
				&mut snapshot,
				"diagnose_tracker_observer_unavailable",
			);

			Ok(snapshot)
		},
	}?;

	refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	let results = write_agent_evidence_snapshot(&snapshot, AgentEvidenceSource::DiagnoseCommand)?;
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
	let Some(config_path) = resolve_private_evidence_config_path(&request, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR>, pass --project <SERVICE_ID>, or register one with `decodex project add <PROJECT_DIR>`."
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

pub(in crate::orchestrator) fn write_cli_output<W>(writer: &mut W, output: &str) -> Result<()>
where
	W: Write,
{
	match writer.write_all(output.as_bytes()).and_then(|()| writer.flush()) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(in crate::orchestrator) fn publish_operator_snapshot(
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

fn resolve_private_evidence_config_path(
	request: &EvidenceRequest<'_>,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if request.config_path.is_some() && request.project_id.is_some() {
		eyre::bail!(
			"Pass either --config <PROJECT_DIR> or --project <SERVICE_ID> for evidence readback, not both."
		);
	}

	if let Some(project_id) = request.project_id {
		return runtime::registered_config_path_for_project_id(state_store, project_id).map(Some);
	}

	resolve_config_path(request.config_path, state_store)
}

fn build_live_status_command_snapshot(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	if let Some(status) =
		orchestrator::active_stored_tracker_backoff_status(state_store, config.service_id())?
	{
		return orchestrator::build_operator_status_snapshot_for_tracker_backoff(
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
		Ok(recovered_state) => hydrate_status_snapshot_state(config, state_store, recovered_state)?,
		Err(error) => {
			if let Some(backoff) =
				tracker_connector_backoff(&error, Instant::now(), "runtime_recovery")
			{
				let status = backoff.to_operator_status(
					config.service_id(),
					OffsetDateTime::now_utc().unix_timestamp(),
				);

				persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

				return orchestrator::build_operator_status_snapshot_for_tracker_backoff(
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
				tracker_connector_backoff(&error, Instant::now(), "operator_status_refresh")
			else {
				return Err(error);
			};
			let status = backoff.to_operator_status(
				config.service_id(),
				OffsetDateTime::now_utc().unix_timestamp(),
			);

			persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

			orchestrator::build_operator_status_snapshot_for_tracker_backoff(
				config,
				state_store,
				limit,
				&status,
			)?
		},
	};

	for warning in snapshot_warnings {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, &warning);
	}

	refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	if !snapshot.connector_backoffs.iter().any(|backoff| backoff.connector == "linear") {
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
					tracker_connector_backoff(&error, Instant::now(), "queue_explain")
				else {
					return Err(error);
				};
				let status = backoff.to_operator_status(
					config.service_id(),
					OffsetDateTime::now_utc().unix_timestamp(),
				);

				persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

				print!("{}", render_tracker_backoff_cli_message("run", &status));

				return Ok(());
			},
		};

	print!("{}", render_queue_explain(config, &queued_candidates));

	Ok(())
}

fn print_operator_status_snapshot(snapshot: &OperatorStatusSnapshot, json: bool) -> Result<()> {
	let output = if json {
		format!("{}\n", serde_json::to_string_pretty(snapshot)?)
	} else {
		render_operator_status(snapshot)
	};
	let stdout = io::stdout();
	let mut stdout = stdout.lock();

	write_cli_output(&mut stdout, &output)
}
