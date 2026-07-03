use crate::orchestrator::execution::{
	self, IssueDispatchMode, IssueRunPlan, IssueTracker, Result, ReviewHandoffContext, RunSummary,
	ServiceConfig, StateStore, TrackerToolBridge, WorkflowDocument,
};

pub(super) fn maybe_execute_deterministic_closeout<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
	review_context: &ReviewHandoffContext,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if issue_run.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(None);
	}

	execution::execute_deterministic_closeout(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		tracker_tool_bridge,
		review_context,
	)?;

	Ok(Some(execution::run_summary_from_issue_run(project.service_id(), issue_run)))
}
