use crate::orchestrator::run_cycle::{
	self, IssueRunPlan, IssueTracker, PrepareIssueRunContext, Result, TrackerIssue, WorktreeSpec,
	prepare::{dispatch, lifecycle},
};

pub(in crate::orchestrator::run_cycle::prepare) struct MaterializeIssueRunAfterLease<
	'a,
	'context,
	T,
> {
	pub(in crate::orchestrator::run_cycle::prepare) context:
		&'a PrepareIssueRunContext<'context, T>,
	pub(in crate::orchestrator::run_cycle::prepare) issue: &'a TrackerIssue,
	pub(in crate::orchestrator::run_cycle::prepare) retained_closeout_worktree:
		Option<WorktreeSpec>,
	pub(in crate::orchestrator::run_cycle::prepare) lease_issue_id: &'a str,
	pub(in crate::orchestrator::run_cycle::prepare) issue_state: String,
	pub(in crate::orchestrator::run_cycle::prepare) attempt_number: i64,
	pub(in crate::orchestrator::run_cycle::prepare) run_id: String,
	pub(in crate::orchestrator::run_cycle::prepare) retry_budget_base: i64,
}

pub(in crate::orchestrator::run_cycle::prepare) fn materialize_issue_run_after_lease<T>(
	request: MaterializeIssueRunAfterLease<'_, '_, T>,
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let worktree = if let Some(worktree) = request.retained_closeout_worktree {
		worktree
	} else {
		request.context.worktree_manager.ensure_worktree_with_hooks(
			&request.issue.identifier,
			request.context.dry_run,
			request.context.workflow.frontmatter().execution().workspace_hooks(),
		)?
	};

	if !request.context.dry_run {
		request.context.state_store.upsert_claimed_worktree(
			request.context.project.service_id(),
			request.lease_issue_id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)?;
	}

	let Some(refreshed_issue) =
		run_cycle::refresh_issue(request.context.tracker, request.lease_issue_id)?
	else {
		return Ok(None);
	};

	if !dispatch::prepare_issue_run_dispatch_allowed(
		request.context,
		&refreshed_issue,
		request.lease_issue_id,
		&worktree.branch_name,
		&worktree.path,
	)? {
		return Ok(None);
	}
	if !request.context.dry_run {
		lifecycle::record_starting_attempt(
			request.context.state_store,
			&request.run_id,
			&request.issue.id,
			request.attempt_number,
		)?;
		run_cycle::clear_terminal_guard_marker(&worktree.path)?;
	}

	let initial_issue_state = request
		.context
		.preferred_initial_issue_state
		.map_or_else(|| refreshed_issue.state.name.clone(), str::to_owned);
	let issue_run = IssueRunPlan {
		issue: refreshed_issue,
		issue_state: request.issue_state,
		initial_issue_state,
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: request.context.dispatch_mode,
		attempt_number: request.attempt_number,
		run_id: request.run_id,
		retry_budget_base: request.retry_budget_base,
	};

	if !request.context.dry_run {
		run_cycle::write_prepare_lifecycle_events(
			request.context.tracker,
			request.context.project,
			request.context.workflow,
			request.context.state_store,
			&issue_run,
		)?;
	}

	Ok(Some(issue_run))
}
