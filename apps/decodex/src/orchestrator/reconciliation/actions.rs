#[allow(clippy::wildcard_imports)] use super::*;

pub(in crate::orchestrator::reconciliation) fn reconcile_superseded_run_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	action: &RunLeaseReconciliation,
	newer_run_id: &str,
	newer_attempt_number: i64,
) -> Result<()> {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		attempt = action.run_attempt.attempt_number(),
		superseded_by_run_id = newer_run_id,
		superseded_by_attempt = newer_attempt_number,
		disposition = "superseded",
		"Reconciling superseded run lease without tracker writeback."
	);

	mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "interrupted")?;

	if let Some(lease) = state_store.lease_for_issue(&action.issue.id)?
		&& lease.run_id() == action.run_attempt.run_id()
	{
		state_store.clear_lease(&action.issue.id)?;
	}

	Ok(())
}

pub(in crate::orchestrator::reconciliation) fn reconcile_retained_review_complete_run_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	action: &RunLeaseReconciliation,
) -> Result<()> {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "retained_review_complete",
		"Reconciling completed retained review run."
	);

	mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "succeeded")?;

	state_store.clear_lease(&action.issue.id)?;

	Ok(())
}

pub(in crate::orchestrator::reconciliation) fn reconcile_not_dispatchable_run_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &RunLeaseReconciliation,
) -> Result<()> {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "not_dispatchable",
		"Reconciling run lease for issue that no longer matches dispatch policy."
	);

	mark_run_attempt_if_active(state_store, action.run_attempt.run_id(), "interrupted")?;

	let worktree_path = action.worktree_mapping.as_ref().map_or_else(
		|| worktree_manager.plan_for_issue(&action.issue.identifier).path,
		|mapping| mapping.worktree_path().to_path_buf(),
	);

	if worktree_path.exists() {
		write_retry_budget_marker(
			&worktree_path,
			action.run_attempt.run_id(),
			action.run_attempt.attempt_number(),
			retry_budget_base_for_issue_worktree(state_store, &action.issue.id, &worktree_path)?,
		)?;
	}

	state_store.clear_lease(&action.issue.id)?;

	Ok(())
}
