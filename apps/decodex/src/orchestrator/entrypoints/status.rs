use std::{io, path::Path, time::Instant};

use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	orchestrator::{self, OperatorStatusSnapshot, entrypoints::output},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

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
	let Some(config_path) = orchestrator::resolve_config_path(config_path, &state_store)? else {
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
	} else if orchestrator::status_should_attempt_operator_snapshot_cache(live) {
		match orchestrator::status_snapshot_from_local_operator_cache(&config, limit) {
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
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		config,
		workflow,
		state_store,
	);
	let mut snapshot_warnings = Vec::new();

	match recovered_state {
		Ok(recovered_state) =>
			orchestrator::hydrate_status_snapshot_state(config, state_store, recovered_state)?,
		Err(error) => {
			if let Some(backoff) =
				orchestrator::tracker_connector_backoff(&error, Instant::now(), "runtime_recovery")
			{
				let status = backoff.to_operator_status(
					config.service_id(),
					OffsetDateTime::now_utc().unix_timestamp(),
				);

				orchestrator::persist_tracker_backoff_state(
					state_store,
					config.service_id(),
					&backoff,
				);

				return orchestrator::build_operator_status_snapshot_for_tracker_backoff(
					config,
					state_store,
					limit,
					&status,
				);
			}

			let warning =
				orchestrator::runtime_recovery_warning("runtime_recovery_unavailable", &error);

			tracing::warn!(
				recovery_error_class = orchestrator::runtime_recovery_error_class(&error),
				"Skipped runtime recovery for operator status; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(warning);
		},
	}

	let mut snapshot = match orchestrator::build_status_command_operator_status_snapshot(
		&tracker,
		config,
		workflow,
		state_store,
		limit,
	) {
		Ok(snapshot) => snapshot,
		Err(error) => {
			let Some(backoff) = orchestrator::tracker_connector_backoff(
				&error,
				Instant::now(),
				"operator_status_refresh",
			) else {
				return Err(error);
			};
			let status = backoff.to_operator_status(
				config.service_id(),
				OffsetDateTime::now_utc().unix_timestamp(),
			);

			orchestrator::persist_tracker_backoff_state(state_store, config.service_id(), &backoff);

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

	orchestrator::refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	if !snapshot.connector_backoffs.iter().any(|backoff| backoff.connector == "linear") {
		orchestrator::clear_tracker_backoff_state_best_effort(state_store, config.service_id());
	}

	Ok(snapshot)
}

fn print_operator_status_snapshot(snapshot: &OperatorStatusSnapshot, json: bool) -> Result<()> {
	let output = if json {
		format!("{}\n", serde_json::to_string_pretty(snapshot)?)
	} else {
		orchestrator::render_operator_status(snapshot)
	};
	let stdout = io::stdout();
	let mut stdout = stdout.lock();

	output::write_cli_output(&mut stdout, &output)
}
