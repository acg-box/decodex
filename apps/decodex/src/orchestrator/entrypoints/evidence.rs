use std::path::PathBuf;

use crate::{
	config::ServiceConfig,
	orchestrator::{self, AgentEvidenceSource, DiagnoseRequest, EvidenceRequest},
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(crate) fn run_diagnose(request: DiagnoseRequest<'_>) -> Result<()> {
	if request.limit == 0 {
		eyre::bail!("`diagnose --limit` must be greater than zero.");
	}

	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = orchestrator::resolve_config_path(request.config_path, &state_store)?
	else {
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

	orchestrator::refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	let results = orchestrator::write_agent_evidence_snapshot(
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
		println!("{}", orchestrator::render_agent_evidence_write_result(&result));
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

	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&readback)?);
	} else {
		print!("{}", orchestrator::render_private_evidence_readback(&readback));
	}

	Ok(())
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

	orchestrator::resolve_config_path(request.config_path, state_store)
}
