use crate::orchestrator::reconciliation::{
	self, IssueDispatchMode, IssueRunPlan, Result, RunLeaseReconciliation, StateStore,
	WorktreeManager, WorktreeSpec,
};

pub(super) fn stalled_reconciliation_issue_run(
	state_store: &StateStore,
	project_id: &str,
	worktree_manager: &WorktreeManager,
	action: &RunLeaseReconciliation,
) -> Result<IssueRunPlan> {
	let worktree = action.worktree_mapping.as_ref().map_or_else(
		|| worktree_manager.plan_for_issue(&action.issue.identifier),
		|mapping| WorktreeSpec {
			branch_name: mapping.branch_name().to_owned(),
			issue_identifier: action.issue.identifier.clone(),
			path: mapping.worktree_path().to_path_buf(),
			reused_existing: true,
		},
	);
	let retry_budget_base = reconciliation::retry_budget_base_for_issue_worktree(
		state_store,
		project_id,
		&action.issue.id,
		&worktree.path,
	)?;

	Ok(IssueRunPlan {
		issue: action.issue.clone(),
		issue_state: reconciliation::planned_issue_state_for_dispatch(
			&action.workflow,
			&action.issue,
			IssueDispatchMode::Retry,
			None,
		),
		initial_issue_state: action.issue.state.name.clone(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: action.run_attempt.attempt_number(),
		run_id: action.run_attempt.run_id().to_owned(),
		retry_budget_base,
	})
}
