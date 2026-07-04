use crate::orchestrator::run_cycle::{
	IssueDispatchMode, IssueTracker, PrepareIssueRunContext, Result, TrackerIssue, WorktreeSpec,
};

pub(in crate::orchestrator::run_cycle::prepare) fn retained_closeout_prepare_worktree<T>(
	context: &PrepareIssueRunContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<Option<WorktreeSpec>>
where
	T: IssueTracker,
{
	if context.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(None);
	}

	let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? else {
		return Ok(None);
	};

	if worktree.project_id() != context.project.service_id()
		|| !worktree.worktree_path().try_exists()?
	{
		return Ok(None);
	}

	Ok(Some(WorktreeSpec {
		branch_name: worktree.branch_name().to_owned(),
		issue_identifier: issue.identifier.clone(),
		path: worktree.worktree_path().to_path_buf(),
		reused_existing: true,
	}))
}
