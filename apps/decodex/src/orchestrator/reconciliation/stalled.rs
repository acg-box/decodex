use crate::{
	orchestrator::{
		reconciliation,
		reconciliation::{
			CONTINUATION_PENDING_RUN_STATUS, Duration, IssueDispatchMode, IssueRunPlan,
			IssueTracker, OffsetDateTime, Path, RUN_OPERATION_RECONCILIATION, Report, Result,
			RetainedPartialProgress, RetryKind, RunAttempt, RunLeaseDisposition,
			RunLeaseReconciliation, ServiceConfig, StalledRunNeedsAttention, StateStore,
			TrackerIssue, WorkflowDocument, WorktreeManager, WorktreeMapping, WorktreeSpec,
		},
	},
	state,
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

	let issue_run = stalled_reconciliation_issue_run(state_store, worktree_manager, action)?;

	write_reconciliation_operation_marker_best_effort(
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

	let issue_run = stalled_reconciliation_issue_run(state_store, worktree_manager, action)?;
	let recovered = match try_recover_stalled_retained_phase_goal(
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

	write_reconciliation_operation_marker_best_effort(
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

fn try_recover_stalled_retained_phase_goal(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
	issue_run: &IssueRunPlan,
) -> Result<bool> {
	write_reconciliation_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_RECONCILIATION,
	);

	let recovery = reconciliation::recover_phase_goal_continuation(
		project,
		workflow,
		state_store,
		issue_run,
		"stalled_run_detected",
		Some("stalled_run_detected"),
	)?;
	let Some(recovery) = recovery else {
		return Ok(false);
	};

	state_store.update_run_status(&issue_run.run_id, CONTINUATION_PENDING_RUN_STATUS)?;
	state_store.clear_lease(&issue.id)?;

	write_stalled_phase_goal_continuation_retry_marker(state_store, workflow, issue_run)?;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue.id,
		issue = issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		source_phase = recovery.source_phase.as_str(),
		next_phase = recovery.next_phase.as_str(),
		"Recovered stalled retained phase goal; scheduling continuation instead of manual attention."
	);

	Ok(true)
}

fn write_stalled_phase_goal_continuation_retry_marker(
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> Result<()> {
	let attempt = u32::try_from(issue_run.attempt_number).unwrap_or(u32::MAX).max(1);
	let delay = reconciliation::retry_delay(RetryKind::Continuation, attempt, workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	reconciliation::write_retry_schedule_for_run(
		state_store,
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
		RetryKind::Continuation,
		retry_ready_at_unix_epoch,
	)
}

fn stalled_reconciliation_issue_run(
	state_store: &StateStore,
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

fn write_reconciliation_operation_marker_best_effort(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) {
	if let Err(error) = state::write_run_operation_marker_preserving_activity(
		worktree_path,
		run_id,
		attempt_number,
		current_operation,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			current_operation,
			worktree_path = %worktree_path.display(),
			"Run operation marker write failed; continuing stalled-run reconciliation."
		);
	}
}
