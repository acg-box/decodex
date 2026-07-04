use crate::orchestrator::run_cycle::{
	self, IssueDispatchMode, IssueTracker, Path, PrepareIssueRunContext, Result,
	RetryIssueStateHint, TrackerIssue, eyre,
};

pub(in crate::orchestrator::run_cycle::prepare) fn prepare_issue_run_dispatch_allowed<T>(
	context: &PrepareIssueRunContext<'_, T>,
	refreshed_issue: &TrackerIssue,
	lease_issue_id: &str,
	worktree_branch_name: &str,
	worktree_path: &Path,
) -> Result<bool>
where
	T: IssueTracker,
{
	let dispatch_allowed = run_cycle::issue_passes_current_dispatch_policy(
		context.tracker,
		refreshed_issue,
		context.project,
		context.workflow,
		context.state_store,
		context.dispatch_mode,
		RetryIssueStateHint {
			preferred_issue_state: context.preferred_issue_state,
			preferred_initial_issue_state: context.preferred_initial_issue_state,
		},
	)?;

	if !dispatch_allowed {
		if !context.dry_run
			&& context.dispatch_mode == IssueDispatchMode::Closeout
			&& let Some(reason) = run_cycle::closeout_dispatch_block_reason(
				context.tracker,
				refreshed_issue,
				context.project,
				context.workflow,
				context.state_store,
			)? {
			eyre::bail!("retained closeout dispatch blocked: {reason}");
		}
		if !context.dry_run && run_cycle::is_terminal_issue(refreshed_issue, context.workflow) {
			run_cycle::cleanup_terminal_worktree(
				context.state_store,
				context.worktree_manager,
				context.workflow,
				lease_issue_id,
				&refreshed_issue.identifier,
				worktree_branch_name,
				worktree_path,
			)?;
		}
	}

	Ok(dispatch_allowed)
}
