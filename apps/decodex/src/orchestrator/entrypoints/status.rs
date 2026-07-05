mod live_snapshot;
mod print;

use std::path::Path;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		OperatorStatusSnapshot, {self},
	},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
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
		let mut snapshot = live_snapshot::build_live_status_command_snapshot(
			&config,
			&workflow,
			&state_store,
			limit,
		)?;

		snapshot.status_source = Some(String::from("live_observers"));

		snapshot
	} else if orchestrator::status_should_attempt_operator_snapshot_cache(live) {
		cached_or_local_status_snapshot(&config, &workflow, &state_store, limit)?
	} else {
		local_status_snapshot(&config, &workflow, &state_store, limit)?
	};

	crate::orchestrator::entrypoints::status::print::print_operator_status_snapshot(&snapshot, json)
}

fn cached_or_local_status_snapshot(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	match orchestrator::status_snapshot_from_local_operator_cache(config, limit) {
		Ok(snapshot) => Ok(snapshot),
		Err(cache_miss) => {
			let mut snapshot = local_status_snapshot(config, workflow, state_store, limit)?;

			orchestrator::add_status_snapshot_cache_miss_warning(&mut snapshot, config, cache_miss);

			Ok(snapshot)
		},
	}
}

fn local_status_snapshot(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot> {
	let mut snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
		config,
		workflow,
		state_store,
		limit,
	)?;

	snapshot.status_source = Some(String::from("local_runtime"));

	Ok(snapshot)
}
