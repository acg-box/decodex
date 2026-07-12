use crate::{
	orchestrator::{
		self, IssueDispatchMode, MaterializedDaemonSpawnState, Result, RunSummary, ServiceConfig,
		StateStore, WorkflowDocument, WorktreeManager, WorktreeSpec,
	},
	prelude::eyre,
};

pub(crate) fn materialize_daemon_spawn_state(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	summary: &RunSummary,
) -> Result<MaterializedDaemonSpawnState> {
	let worktree = materialize_run_summary_worktree(project, workflow, summary)?;
	let retry_budget_base = orchestrator::retry_budget_base_for_dispatch_mode(
		state_store,
		project.service_id(),
		&summary.issue_id,
		&worktree.path,
		summary.dispatch_mode,
		None,
	)?;

	Ok(MaterializedDaemonSpawnState { worktree, retry_budget_base })
}

pub(crate) fn materialize_run_summary_worktree(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	summary: &RunSummary,
) -> Result<WorktreeSpec> {
	if summary.dispatch_mode == IssueDispatchMode::Closeout {
		if !summary.worktree_path.try_exists()? {
			eyre::bail!(
				"planned retained closeout worktree `{}` is missing for issue `{}`",
				summary.worktree_path.display(),
				summary.issue_identifier
			);
		}

		return Ok(WorktreeSpec {
			branch_name: summary.branch_name.clone(),
			issue_identifier: summary.issue_identifier.clone(),
			path: summary.worktree_path.clone(),
			reused_existing: true,
		});
	}

	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let worktree = worktree_manager.ensure_worktree_with_hooks(
		&summary.issue_identifier,
		false,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;

	if worktree.path != summary.worktree_path {
		eyre::bail!(
			"planned worktree path `{}` diverged from materialized path `{}` for issue `{}`",
			summary.worktree_path.display(),
			worktree.path.display(),
			summary.issue_identifier
		);
	}
	if worktree.branch_name != summary.branch_name {
		eyre::bail!(
			"planned branch `{}` diverged from materialized branch `{}` for issue `{}`",
			summary.branch_name,
			worktree.branch_name,
			summary.issue_identifier
		);
	}

	Ok(worktree)
}
