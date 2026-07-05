mod closeout;
mod review;
mod stale;

pub(crate) use self::closeout::retained_closeout_lease_has_fresh_activity;

use crate::orchestrator::run_cycle_reconciliation::{
	self, HashMap, HashSet, IssueLease, IssueTracker, ProjectStateReconciliationContext, Result,
	ServiceConfig, StateStore, TrackerIssue, WorkflowDocument, leases, tracker,
};

pub(super) fn reconcile_active_project_leases<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	for lease in leases {
		if leases::review::reconcile_success_retained_review_lease(context, lease, issues_by_id)? {
			continue;
		}
		if leases::closeout::reconcile_terminal_retained_closeout_lease(
			context,
			lease,
			issues_by_id,
			now_unix_epoch,
			cleared_terminal_lane_issue_ids,
		)? {
			continue;
		}

		leases::stale::reconcile_stale_project_lease(
			context,
			lease,
			issues_by_id,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	Ok(())
}

pub(super) fn clear_terminal_lane_labels_once<T>(
	tracker: &T,
	project: &ServiceConfig,
	issue: &TrackerIssue,
	cleared_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	if cleared_issue_ids.insert(issue.id.clone()) {
		tracker::clear_automation_lane_labels(tracker, issue, project.service_id())?;
	}

	Ok(())
}

pub(crate) fn terminal_issue_keeps_retained_closeout<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	if !run_cycle_reconciliation::is_terminal_issue(issue, workflow) {
		return Ok(false);
	}

	Ok(run_cycle_reconciliation::issue_passes_closeout_dispatch_policy(
		tracker,
		issue,
		project,
		workflow,
		state_store,
	)? || run_cycle_reconciliation::closeout_dispatch_block_reason(
		tracker,
		issue,
		project,
		workflow,
		state_store,
	)?
	.is_some())
}
