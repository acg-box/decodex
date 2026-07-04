use std::collections::{HashMap, HashSet};

use crate::{
	orchestrator::{
		self, IssueTracker, run_cycle_reconciliation,
		run_cycle_reconciliation::ProjectStateReconciliationContext,
	},
	prelude::Result,
	state::WorktreeMapping,
	tracker::TrackerIssue,
};

pub(in crate::orchestrator::run_cycle_reconciliation) fn cleanup_terminal_project_worktrees<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	for mapping in worktrees {
		if let Some(issue) = issues_by_id.get(mapping.issue_id())
			&& orchestrator::is_terminal_issue(issue, context.workflow)
			&& !orchestrator::terminal_issue_keeps_retained_closeout(
				context.tracker,
				issue,
				context.project,
				context.workflow,
				context.state_store,
			)? {
			run_cycle_reconciliation::clear_terminal_lane_labels_once(
				context.tracker,
				context.project,
				issue,
				cleared_terminal_lane_issue_ids,
			)?;
			orchestrator::cleanup_worktree_mapping(
				context.state_store,
				context.worktree_manager,
				context.workflow,
				&issue.identifier,
				mapping,
			)?;
		}
	}

	Ok(())
}
