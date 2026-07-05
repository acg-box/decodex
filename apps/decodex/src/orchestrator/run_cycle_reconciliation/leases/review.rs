use crate::orchestrator::run_cycle_reconciliation::{
	self, HashMap, IssueLease, IssueTracker, ProjectStateReconciliationContext, Result, StateStore,
	TrackerIssue,
};

pub(in crate::orchestrator::run_cycle_reconciliation::leases) fn reconcile_success_retained_review_lease<
	T,
>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<bool>
where
	T: IssueTracker,
{
	if let Some(issue) = issues_by_id.get(lease.issue_id())
		&& issue.state.name == context.workflow.frontmatter().tracker().success_state()
		&& retained_review_lease_matches_run(context.state_store, lease)?
	{
		run_cycle_reconciliation::mark_run_attempt_if_active(
			context.state_store,
			lease.run_id(),
			"succeeded",
		)?;

		context.state_store.clear_lease(lease.issue_id())?;

		return Ok(true);
	}

	Ok(false)
}

fn retained_review_lease_matches_run(state_store: &StateStore, lease: &IssueLease) -> Result<bool> {
	let Some(run_attempt) = state_store.run_attempt(lease.run_id())? else {
		return Ok(false);
	};
	let worktree_mapping = state_store.worktree_for_issue(lease.issue_id())?;

	run_cycle_reconciliation::retained_review_handoff_matches_run(
		state_store,
		&run_attempt,
		worktree_mapping.as_ref(),
	)
}
