use std::time::Instant;

use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	orchestrator::{self, OperatorStatusSnapshot},
	prelude::Result,
	state::StateStore,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(in crate::orchestrator::entrypoints::status) fn build_live_status_command_snapshot(
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

	let mut snapshot =
		refreshed_operator_status_snapshot(&tracker, config, workflow, state_store, limit)?;

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

fn refreshed_operator_status_snapshot(
	tracker: &LinearClient,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	match orchestrator::build_status_command_operator_status_snapshot(
		tracker,
		config,
		workflow,
		state_store,
		limit,
	) {
		Ok(snapshot) => Ok(snapshot),
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
			)
		},
	}
}
