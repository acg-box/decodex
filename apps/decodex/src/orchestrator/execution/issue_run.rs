mod inner;

pub(crate) use self::inner::execute_issue_run_inner;

use crate::orchestrator::{
	self, IssueRunPlan, IssueTracker, Result, RunSummary, ServiceConfig, StateStore,
	WorkflowDocument, execution::resume,
};

pub(crate) fn execute_issue_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: IssueRunPlan,
) -> Result<RunSummary>
where
	T: IssueTracker,
{
	tracing::info!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		branch = issue_run.worktree.branch_name,
		worktree_path = %orchestrator::relative_worktree_path(project, &issue_run.worktree),
		"Starting issue run."
	);

	state_store.upsert_claimed_worktree(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
		&issue_run.worktree.path.display().to_string(),
	)?;

	let result = orchestrator::ensure_automation_activity_label(
		tracker,
		&issue_run.issue,
		project.service_id(),
		true,
	)
	.and_then(|_| {
		inner::execute_issue_run_inner(tracker, project, workflow, state_store, &issue_run)
	});

	state_store.clear_lease(&issue_run.issue.id)?;

	match result {
		Ok(summary) => {
			resume::persist_issue_run_outcome(state_store, &issue_run.run_id, &summary)?;

			if !summary.continuation_pending {
				state_store.clear_loop_guardrail_checkpoints_for_issue(
					project.service_id(),
					&issue_run.issue.id,
				)?;

				orchestrator::reconcile_terminal_thread_archive_backlog_best_effort(
					project,
					workflow,
					state_store,
				);
			}

			tracing::info!(
				project_id = project.service_id(),
				issue_id = issue_run.issue.id,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				attempt = issue_run.attempt_number,
				branch = issue_run.worktree.branch_name,
				worktree_path = %orchestrator::relative_worktree_path(project, &issue_run.worktree),
				"Completed issue run."
			);

			Ok(summary)
		},
		Err(error) => {
			state_store.update_run_status(&issue_run.run_id, "failed")?;

			orchestrator::handle_failure(
				tracker,
				project,
				workflow,
				state_store,
				&issue_run,
				&error,
			)?;

			Err(error)
		},
	}
}
