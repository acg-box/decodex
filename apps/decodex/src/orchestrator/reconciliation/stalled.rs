mod markers;
mod phase_goal;
mod plan;

use crate::orchestrator::reconciliation::{
	self, Duration, IssueTracker, RUN_OPERATION_RECONCILIATION, Report, Result,
	RetainedPartialProgress, RunAttempt, RunLeaseDisposition, RunLeaseReconciliation,
	ServiceConfig, StalledRunNeedsAttention, StateStore, WorktreeManager, WorktreeMapping,
};

pub(crate) fn reconcile_stalled_run_lease<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &RunLeaseReconciliation,
	idle_for: Duration,
) -> Result<()>
where
	T: IssueTracker,
{
	tracing::warn!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "stalled",
		idle_for_s = idle_for.as_secs(),
		"Reconciling stalled run."
	);

	state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;
	state_store.clear_lease(&action.issue.id)?;

	let issue_run = plan::stalled_reconciliation_issue_run(state_store, worktree_manager, action)?;

	markers::write_reconciliation_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_RECONCILIATION,
	);
	reconciliation::handle_failure(
		tracker,
		project,
		&action.workflow,
		state_store,
		&issue_run,
		&Report::new(StalledRunNeedsAttention {
			issue_identifier: action.issue.identifier.clone(),
			run_id: action.run_attempt.run_id().to_owned(),
			idle_for,
		}),
	)?;

	Ok(())
}

pub(crate) fn reconcile_stalled_retained_partial_progress_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	action: &RunLeaseReconciliation,
	idle_for: Duration,
) -> Result<()>
where
	T: IssueTracker,
{
	tracing::warn!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "stalled_retained_partial_progress",
		idle_for_s = idle_for.as_secs(),
		"Reconciling stalled run with retained partial progress."
	);

	let issue_run = plan::stalled_reconciliation_issue_run(state_store, worktree_manager, action)?;
	let recovered = match phase_goal::try_recover_stalled_retained_phase_goal(
		project,
		&action.workflow,
		state_store,
		&action.issue,
		&issue_run,
	) {
		Ok(recovered) => recovered,
		Err(error) if reconciliation::run_failure_requires_terminal_attention(&error) => {
			state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;
			state_store.clear_lease(&action.issue.id)?;

			reconciliation::handle_failure(
				tracker,
				project,
				&action.workflow,
				state_store,
				&issue_run,
				&error,
			)?;

			return Ok(());
		},
		Err(error) => return Err(error),
	};

	if recovered {
		return Ok(());
	}

	state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;
	state_store.clear_lease(&action.issue.id)?;

	let worktree_path = reconciliation::relative_worktree_path(project, &issue_run.worktree);

	markers::write_reconciliation_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_RECONCILIATION,
	);
	reconciliation::handle_failure(
		tracker,
		project,
		&action.workflow,
		state_store,
		&issue_run,
		&Report::new(RetainedPartialProgress {
			issue_identifier: action.issue.identifier.clone(),
			run_id: action.run_attempt.run_id().to_owned(),
			worktree_path,
			source_error_class: Some(String::from("stalled_run_detected")),
		}),
	)?;

	Ok(())
}

pub(crate) fn reconcile_stalled_attention_run_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	action: &RunLeaseReconciliation,
	idle_for: Duration,
) -> Result<()> {
	tracing::warn!(
		project_id = project.service_id(),
		issue_id = action.issue.id,
		issue = action.issue.identifier,
		run_id = action.run_attempt.run_id(),
		disposition = "stalled_already_needs_attention",
		idle_for_s = idle_for.as_secs(),
		"Reconciling stalled run that is already blocked for operator attention."
	);

	state_store.update_run_status(action.run_attempt.run_id(), "stalled")?;

	state_store.clear_lease(&action.issue.id)
}

pub(crate) fn stalled_run_has_retained_partial_progress(
	worktree_mapping: Option<&WorktreeMapping>,
) -> bool {
	match worktree_mapping {
		Some(mapping) => reconciliation::worktree_has_tracked_changes(mapping.worktree_path()),
		None => false,
	}
}

pub(crate) fn retained_review_handoff_matches_run(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<bool> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(false);
	};
	let Some(marker) = state_store.review_handoff_marker(
		worktree_mapping.project_id(),
		run_attempt.issue_id(),
		worktree_mapping.branch_name(),
	)?
	else {
		return Ok(false);
	};

	Ok(marker.run_id() == run_attempt.run_id()
		&& marker.attempt_number() == run_attempt.attempt_number()
		&& marker.branch_name() == worktree_mapping.branch_name())
}

pub(crate) fn superseded_run_disposition(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
) -> Result<Option<RunLeaseDisposition>> {
	let Some(latest_attempt) = state_store.latest_run_attempt_for_issue(run_attempt.issue_id())?
	else {
		return Ok(None);
	};

	if latest_attempt.attempt_number() <= run_attempt.attempt_number() {
		return Ok(None);
	}

	Ok(Some(RunLeaseDisposition::Superseded {
		newer_run_id: latest_attempt.run_id().to_owned(),
		newer_attempt_number: latest_attempt.attempt_number(),
	}))
}
