mod candidate;
mod execution;
mod planning;
mod queue;

pub(crate) use planning::plan_project_issue_run_with_exclusions;

use crate::orchestrator::run_cycle::{
	IssueTracker, Result, RunSummary, ServiceConfig, StateStore, WorkflowDocument,
};

pub(crate) fn run_project_once<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	execution::run_project_once_with_exclusions(
		tracker,
		project,
		workflow,
		state_store,
		dry_run,
		&[],
	)
}
