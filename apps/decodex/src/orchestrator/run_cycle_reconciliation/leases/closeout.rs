use crate::{
	orchestrator::{
		run_cycle_reconciliation,
		run_cycle_reconciliation::{
			HashMap, HashSet, IssueLease, IssueTracker, ProjectStateReconciliationContext, Result,
			ServiceConfig, TrackerIssue, WorktreeManager,
		},
	},
	state,
};

pub(in crate::orchestrator::run_cycle_reconciliation::leases) fn reconcile_terminal_retained_closeout_lease<
	T,
>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let Some(issue) = issues_by_id.get(lease.issue_id()) else {
		return Ok(false);
	};

	if !run_cycle_reconciliation::terminal_issue_keeps_retained_closeout(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
	)? {
		return Ok(false);
	}
	if retained_closeout_lease_has_fresh_activity(
		lease.run_id(),
		issue,
		context.project,
		now_unix_epoch,
	)? {
		return Ok(true);
	}

	run_cycle_reconciliation::clear_terminal_lane_labels_once(
		context.tracker,
		context.project,
		issue,
		cleared_terminal_lane_issue_ids,
	)?;
	run_cycle_reconciliation::mark_run_attempt_if_active(
		context.state_store,
		lease.run_id(),
		"interrupted",
	)?;

	context.state_store.clear_lease(lease.issue_id())?;

	Ok(true)
}

pub(crate) fn retained_closeout_lease_has_fresh_activity(
	run_id: &str,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	now_unix_epoch: i64,
) -> Result<bool> {
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let worktree = worktree_manager.plan_for_issue(&issue.identifier);
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == run_id
		&& run_cycle_reconciliation::worktree_activity_marker_is_fresh(&marker, now_unix_epoch))
}
