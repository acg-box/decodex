use crate::orchestrator::{
	CachedWorkflowDocument, DaemonTickContext, LinearClient, Path, Result, ServiceConfig,
	WorkflowDocument, WorktreeManager,
};

pub(crate) fn load_daemon_tick_context(
	config_path: &Path,
	workflow_cache: &mut Option<CachedWorkflowDocument>,
) -> Result<DaemonTickContext> {
	let config = ServiceConfig::from_path(config_path)?;
	let workflow = load_daemon_tick_workflow(&config, workflow_cache)?;
	let api_key = config.tracker().resolve_api_key()?;
	let tracker = LinearClient::new(api_key)?;
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	Ok(DaemonTickContext { config, workflow, tracker, worktree_manager })
}

pub(crate) fn load_daemon_tick_workflow(
	config: &ServiceConfig,
	workflow_cache: &mut Option<CachedWorkflowDocument>,
) -> Result<WorkflowDocument> {
	let workflow_path = config.workflow_path().to_path_buf();
	let cached_same_path = workflow_cache
		.as_ref()
		.filter(|cached| cached.path == workflow_path)
		.map(|cached| cached.document.clone());

	match WorkflowDocument::from_path(&workflow_path) {
		Ok(workflow) => {
			if cached_same_path.as_ref().is_some_and(|cached| cached != &workflow) {
				tracing::info!(
					workflow_path = %workflow_path.display(),
					"Reloaded project WORKFLOW.md for future control-plane decisions."
				);
			}

			*workflow_cache =
				Some(CachedWorkflowDocument { path: workflow_path, document: workflow.clone() });

			Ok(workflow)
		},
		Err(error) =>
			if let Some(cached_workflow) = cached_same_path {
				tracing::warn!(
					workflow_path = %workflow_path.display(),
					?error,
					"Failed to reload project WORKFLOW.md; keeping the last known good workflow active for control-plane decisions."
				);

				Ok(cached_workflow)
			} else {
				Err(error)
			},
	}
}
