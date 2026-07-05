use crate::orchestrator::run_cycle::{
	self, IssueTracker, Result, RunSummary, ServiceConfig, StateStore, WorkflowDocument,
	project::planning,
};

pub(in crate::orchestrator::run_cycle::project) fn run_project_once_with_exclusions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let Some(issue_run) = planning::plan_project_issue_run_with_exclusions(
		tracker,
		project,
		workflow,
		state_store,
		dry_run,
		excluded_issue_ids,
	)?
	else {
		if !dry_run {
			run_cycle::reconcile_terminal_thread_archive_backlog_best_effort(
				project,
				workflow,
				state_store,
			);
		}

		return Ok(None);
	};

	run_cycle::complete_issue_run(tracker, project, workflow, state_store, issue_run, dry_run)
}
