use crate::{
	orchestrator::{
		self, IssueTracker, Result, RunLeaseDisposition, RunLeaseReconciliation, ServiceConfig,
		StateStore, WorktreeManager,
		reconciliation::{actions, stalled},
	},
	tracker,
};

pub(crate) fn apply_run_lease_reconciliation<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	actions: Vec<RunLeaseReconciliation>,
) -> Result<()>
where
	T: IssueTracker,
{
	for action in actions {
		match &action.disposition {
			RunLeaseDisposition::RetainedReviewComplete => {
				actions::reconcile_retained_review_complete_run_lease(
					project,
					state_store,
					&action,
				)?;
			},
			RunLeaseDisposition::Superseded { newer_run_id, newer_attempt_number } => {
				actions::reconcile_superseded_run_lease(
					project,
					state_store,
					&action,
					newer_run_id,
					*newer_attempt_number,
				)?;
			},
			RunLeaseDisposition::Terminal => {
				reconcile_terminal_run_lease(
					tracker,
					project,
					state_store,
					worktree_manager,
					&action,
				)?;
			},
			RunLeaseDisposition::NotDispatchable => {
				actions::reconcile_not_dispatchable_run_lease(
					project,
					state_store,
					worktree_manager,
					&action,
				)?;
			},
			RunLeaseDisposition::Stalled { idle_for } => {
				stalled::reconcile_stalled_run_lease(
					tracker,
					project,
					state_store,
					worktree_manager,
					&action,
					*idle_for,
				)?;
			},
			RunLeaseDisposition::StalledRetainedPartialProgress { idle_for } => {
				stalled::reconcile_stalled_retained_partial_progress_run(
					tracker,
					project,
					state_store,
					worktree_manager,
					&action,
					*idle_for,
				)?;
			},
			RunLeaseDisposition::StalledAlreadyNeedsAttention { idle_for } => {
				stalled::reconcile_stalled_attention_run_lease(
					project,
					state_store,
					&action,
					*idle_for,
				)?;
			},
		}
	}

	Ok(())
}

fn reconcile_terminal_run_lease<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &RunLeaseReconciliation,
) -> Result<()>
where
	T: IssueTracker,
{
	tracing::info!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "terminal",
		"Reconciling terminal run lease."
	);

	orchestrator::mark_run_attempt_if_active(
		state_store,
		action.run_attempt.run_id(),
		"terminated",
	)?;
	tracker::clear_automation_lane_labels(tracker, &action.issue, project.service_id())?;

	state_store.clear_lease(&action.issue.id)?;

	if let Some(mapping) = &action.worktree_mapping {
		orchestrator::cleanup_worktree_mapping(
			state_store,
			worktree_manager,
			&action.workflow,
			&action.issue.identifier,
			mapping,
		)?;
	}

	Ok(())
}
