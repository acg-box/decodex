mod closeout;
mod dispatch;
mod identity;
mod lifecycle;
mod materialize;

use crate::orchestrator::run_cycle::{
	self, IssueRunPlan, IssueTracker, PrepareIssueRunContext, Result, TrackerIssue,
	prepare::materialize::MaterializeIssueRunAfterLease,
};

pub(crate) fn prepare_issue_run<T>(
	context: PrepareIssueRunContext<'_, T>,
	issue: TrackerIssue,
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let retained_closeout_worktree =
		closeout::retained_closeout_prepare_worktree(&context, &issue)?;
	let planned_worktree = retained_closeout_worktree
		.clone()
		.unwrap_or_else(|| context.worktree_manager.plan_for_issue(&issue.identifier));
	let Some((attempt_number, run_id)) = identity::resolve_prepare_run_identity(
		context.state_store,
		context.project.service_id(),
		&issue,
		context.preferred_run_identity,
	)?
	else {
		return Ok(None);
	};
	let retry_budget_base = run_cycle::retry_budget_base_for_dispatch_mode(
		context.state_store,
		context.project.service_id(),
		&issue.id,
		&planned_worktree.path,
		context.dispatch_mode,
		context.preferred_retry_budget_base,
	)?;
	let lease_issue_id = issue.id.clone();
	let issue_state = run_cycle::planned_issue_state_for_dispatch(
		context.workflow,
		&issue,
		context.dispatch_mode,
		context.preferred_issue_state,
	);

	run_cycle::validate_workflow_read_first_files(context.project, context.workflow)?;

	if !context.dry_run
		&& !context.lease_preacquired
		&& !context.state_store.try_acquire_registered_lease(
			context.project.service_id(),
			&issue.id,
			&run_id,
			&issue_state,
		)? {
		return Ok(None);
	}

	match materialize::materialize_issue_run_after_lease(MaterializeIssueRunAfterLease {
		context: &context,
		issue: &issue,
		retained_closeout_worktree,
		lease_issue_id: &lease_issue_id,
		issue_state,
		attempt_number,
		run_id,
		retry_budget_base,
	}) {
		Ok(Some(issue_run)) => Ok(Some(issue_run)),
		Ok(None) => {
			lifecycle::clear_prepare_issue_run_lease(
				context.state_store,
				context.dry_run,
				&lease_issue_id,
			)?;

			Ok(None)
		},
		Err(error) => {
			lifecycle::clear_prepare_issue_run_lease(
				context.state_store,
				context.dry_run,
				&lease_issue_id,
			)?;

			Err(error)
		},
	}
}
