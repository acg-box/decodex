use crate::orchestrator::run_cycle_reconciliation::{
	self, HashMap, HashSet, IssueTracker, LaneClaim, ProjectStateReconciliationContext, Result,
	TrackerIssue,
};

pub(in crate::orchestrator::run_cycle_reconciliation::leases) fn reconcile_stale_project_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	claim: &LaneClaim,
	issues_by_id: &HashMap<String, TrackerIssue>,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	let reconciled_status = match issues_by_id.get(claim.id().tracker_issue_id()) {
		Some(issue) if run_cycle_reconciliation::is_terminal_issue(issue, context.workflow) =>
			"terminated",
		Some(_) | None => "interrupted",
	};

	if let Some(issue) = issues_by_id.get(claim.id().tracker_issue_id())
		&& run_cycle_reconciliation::is_terminal_issue(issue, context.workflow)
	{
		run_cycle_reconciliation::clear_terminal_lane_labels_once(
			context.tracker,
			context.project,
			issue,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	run_cycle_reconciliation::mark_run_attempt_if_active(
		context.state_store,
		claim.run_id(),
		reconciled_status,
	)?;

	context
		.state_store
		.release_lane_claim(
			context.project.service_id(),
			claim.id().tracker_issue_id(),
			claim.run_id(),
		)
		.map(|_| ())
}
