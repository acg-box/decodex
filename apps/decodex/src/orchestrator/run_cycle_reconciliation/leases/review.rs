use crate::orchestrator::run_cycle_reconciliation::{
	self, HashMap, IssueTracker, LaneClaim, ProjectStateReconciliationContext, Result, StateStore,
	TrackerIssue,
};

pub(in crate::orchestrator::run_cycle_reconciliation::leases) fn reconcile_success_retained_review_lease<
	T,
>(
	context: &ProjectStateReconciliationContext<'_, T>,
	claim: &LaneClaim,
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<bool>
where
	T: IssueTracker,
{
	if let Some(issue) = issues_by_id.get(claim.id().tracker_issue_id())
		&& issue.state.name == context.workflow.frontmatter().tracker().success_state()
		&& retained_review_claim_matches_run(context.state_store, claim)?
	{
		run_cycle_reconciliation::mark_run_attempt_if_active(
			context.state_store,
			claim.run_id(),
			"succeeded",
		)?;

		context.state_store.release_lane_claim(
			context.project.service_id(),
			claim.id().tracker_issue_id(),
			claim.run_id(),
		)?;

		return Ok(true);
	}

	Ok(false)
}

fn retained_review_claim_matches_run(state_store: &StateStore, claim: &LaneClaim) -> Result<bool> {
	let Some(run_attempt) = state_store.run_attempt(claim.run_id())? else {
		return Ok(false);
	};
	let worktree_mapping = state_store.worktree_for_issue(claim.id().tracker_issue_id())?;

	run_cycle_reconciliation::retained_review_handoff_matches_run(
		state_store,
		&run_attempt,
		worktree_mapping.as_ref(),
	)
}
