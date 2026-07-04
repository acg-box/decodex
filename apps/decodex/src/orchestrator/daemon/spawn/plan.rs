use crate::orchestrator::{
	self, IssueTracker, Result, RetryDispatchDecision, RetryQueue, RunSummary, ServiceConfig,
	StateStore, WorkflowDocument, daemon,
};

pub(crate) fn plan_next_daemon_run<T>(
	retry_queue: &mut RetryQueue,
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Option<(RunSummary, bool)>>
where
	T: IssueTracker,
{
	match daemon::plan_due_retry_run(retry_queue, tracker, project, workflow, state_store)? {
		RetryDispatchDecision::Dispatch(summary) => Ok(Some((*summary, true))),
		RetryDispatchDecision::Blocked { excluded_issue_ids } => {
			let excluded_issue_ids =
				excluded_issue_ids.iter().map(String::as_str).collect::<Vec<_>>();
			let issue_run = orchestrator::plan_project_issue_run_with_exclusions(
				tracker,
				project,
				workflow,
				state_store,
				true,
				&excluded_issue_ids,
			)?;

			Ok(issue_run.map(|issue_run| {
				(orchestrator::run_summary_from_issue_run(project.service_id(), &issue_run), false)
			}))
		},
		RetryDispatchDecision::Continue => {
			let issue_run = orchestrator::plan_project_issue_run_with_exclusions(
				tracker,
				project,
				workflow,
				state_store,
				true,
				&[],
			)?;

			Ok(issue_run.map(|issue_run| {
				(orchestrator::run_summary_from_issue_run(project.service_id(), &issue_run), false)
			}))
		},
	}
}
