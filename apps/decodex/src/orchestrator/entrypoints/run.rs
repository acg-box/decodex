use std::time::Instant;

use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	orchestrator::{self, PreferredRunIdentity, RunCycleRequest, RunOnceRequest},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(crate) fn run_once(request: RunOnceRequest<'_>) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = orchestrator::resolve_config_path(request.config_path, &state_store)?
	else {
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
	let workflow =
		orchestrator::load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;

	if let Some(status) =
		orchestrator::active_stored_tracker_backoff_status(&state_store, config.service_id())?
	{
		print!("{}", orchestrator::render_tracker_backoff_cli_message("run", &status));

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

	let run_summary = match orchestrator::run_configured_cycle(RunCycleRequest {
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
			let Some(backoff) =
				orchestrator::tracker_connector_backoff(&error, Instant::now(), "run_cycle")
			else {
				return Err(error);
			};
			let status = backoff.to_operator_status(
				config.service_id(),
				OffsetDateTime::now_utc().unix_timestamp(),
			);

			orchestrator::persist_tracker_backoff_state(
				&state_store,
				config.service_id(),
				&backoff,
			);

			print!("{}", orchestrator::render_tracker_backoff_cli_message("run", &status));

			return Ok(());
		},
	};

	if let Some(summary) = run_summary {
		orchestrator::clear_tracker_backoff_state_best_effort(&state_store, config.service_id());

		println!("{}", orchestrator::format_run_once_summary(&summary, request.dry_run));

		return Ok(());
	}

	orchestrator::clear_tracker_backoff_state_best_effort(&state_store, config.service_id());

	println!("{}", orchestrator::format_no_eligible_issue_message(&config, &workflow));

	Ok(())
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
	let queued_candidates = match orchestrator::build_queued_candidate_statuses(
		&tracker,
		config,
		workflow,
		state_store,
	) {
		Ok(queued_candidates) => queued_candidates,
		Err(error) => {
			let Some(backoff) =
				orchestrator::tracker_connector_backoff(&error, Instant::now(), "queue_explain")
			else {
				return Err(error);
			};
			let status = backoff.to_operator_status(
				config.service_id(),
				OffsetDateTime::now_utc().unix_timestamp(),
			);

			orchestrator::persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

			print!("{}", orchestrator::render_tracker_backoff_cli_message("run", &status));

			return Ok(());
		},
	};

	print!("{}", orchestrator::render_queue_explain(config, &queued_candidates));

	Ok(())
}
