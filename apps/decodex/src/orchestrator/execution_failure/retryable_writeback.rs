use super::{
	AppServerCapabilityPreflightFailure, AppServerPhaseGoalFailure, AppServerTransportFailure,
	AppServerZeroEvidenceStartFailure, FailureHandlingContext, HarnessOutcomeKind,
	IssueDispatchMode, IssueRunPlan, IssueTracker, OffsetDateTime,
	RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, RepoGateFailure, Report, Result, RetryComment,
	RetryKind, StalledRunNeedsAttention, WorkflowDocument, ensure_automation_activity_label,
	format_retry_comment, json, latest_open_issue_phase_goal_before_attempt,
	loop_guardrail_worktree_fingerprint, record_harness_outcome_best_effort,
	retained_progress_source_error_class, retry_comment_details, retry_delay, state, tracker,
	write_retry_budget_marker,
};

pub(super) fn apply_retryable_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
	max_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let (retry_error_class, retry_next_action) = retry_comment_details(error);

	write_retry_schedule_marker_for_runtime_retry(
		error,
		context.workflow,
		context.issue_run,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		retry_budget_attempt = context.retry_budget_attempts,
		max_attempts,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		error_class = retry_error_class,
		"Run failed and remains retryable."
	);

	tracker::create_public_comment(
		context.tracker,
		&context.issue_run.issue.id,
		&format_retry_comment(RetryComment {
			run_id: &context.issue_run.run_id,
			attempt_number: context.issue_run.attempt_number,
			retry_budget_attempt_number: context.retry_budget_attempts,
			max_attempts,
			worktree_path: context.worktree_path.to_owned(),
			branch_name: &context.issue_run.worktree.branch_name,
			error_class: retry_error_class,
			next_action: &retry_next_action,
		}),
	)?;

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;
	record_harness_outcome_best_effort(
		context.state_store,
		context.project.service_id(),
		context.issue_run,
		HarnessOutcomeKind::RetryableFailure,
		Some(retry_error_class),
		retryable_failure_validation_result(error, retry_error_class),
		None,
	);
	cleanup_retryable_failed_start_ownership(context, error)?;

	Ok(())
}

pub(in crate::orchestrator) fn write_retry_schedule_marker_for_runtime_retry(
	error: &Report,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}
	if error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
	{
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}
	if error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.is_some_and(AppServerPhaseGoalFailure::is_terminal_path_missing)
	{
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}

	let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() else {
		return Ok(());
	};
	let Some(retry_kind) = repo_gate_failure.retry_schedule_kind() else {
		return Ok(());
	};

	write_retry_schedule_marker(workflow, issue_run, retry_budget_attempts, retry_kind)
}

fn cleanup_retryable_failed_start_ownership<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	if !retryable_failed_start_cleanup_allowed(context, error)? {
		return Ok(());
	}

	let tracker_policy = context.workflow.frontmatter().tracker();
	let failure_state_name = tracker_policy.failure_state();
	let failure_state_is_startable =
		tracker_policy.startable_states().iter().any(|state| state == failure_state_name);

	if !failure_state_is_startable {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the configured failure state is not startable."
		);

		return Ok(());
	}

	let Some(state_id) = context.issue_run.issue.state_id_for_name(failure_state_name) else {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the target state id was not available."
		);

		return Ok(());
	};

	context.tracker.update_issue_state(&context.issue_run.issue.id, state_id)?;

	ensure_automation_activity_label(
		context.tracker,
		&context.issue_run.issue,
		context.project.service_id(),
		false,
	)?;

	context.state_store.clear_worktree(&context.issue_run.issue.id)?;
	context
		.state_store
		.append_private_execution_event(
			context.project.service_id(),
			&context.issue_run.issue.id,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
			RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE,
			json!({
				"schema": "decodex.retryable_failed_start_cleanup/1",
				"source_error_class": retained_progress_source_error_class(error)
					.unwrap_or("retryable_execution_failure"),
				"dispatch_mode": context.issue_run.dispatch_mode.as_str(),
				"active_label_cleared": true,
				"worktree_mapping_cleared": true,
				"target_issue_state": failure_state_name,
				"issue_state_reset": true,
				"retryable_by_next_program_pass": true,
			}),
		)
		.map(|_| ())?;

	tracing::info!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		issue_state_reset = true,
		"Cleared retryable failed-start ownership after a no-diff Program run failure."
	);

	Ok(())
}

fn retryable_failed_start_cleanup_allowed<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.issue_run.dispatch_mode != IssueDispatchMode::Program {
		return Ok(false);
	}
	if !retryable_failure_happened_before_effective_agent_execution(error) {
		return Ok(false);
	}
	if context.state_store.lease_for_issue(&context.issue_run.issue.id)?.is_some() {
		return Ok(false);
	}
	if context.state_store.issue_has_review_lifecycle_record(
		context.project.service_id(),
		&context.issue_run.issue.id,
	)? {
		return Ok(false);
	}
	if latest_open_issue_phase_goal_before_attempt(
		context.project,
		context.state_store,
		&context.issue_run.issue.id,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
	)?
	.is_some()
	{
		return Ok(false);
	}

	Ok(loop_guardrail_worktree_fingerprint(&context.issue_run.worktree.path)?
		.is_some_and(|fingerprint| !fingerprint.effective_delta_present))
}

fn retryable_failure_happened_before_effective_agent_execution(error: &Report) -> bool {
	error.downcast_ref::<AppServerZeroEvidenceStartFailure>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error
			.downcast_ref::<AppServerTransportFailure>()
			.is_some_and(AppServerTransportFailure::is_retryable_startup)
}

fn retryable_failure_validation_result(
	error: &Report,
	retry_error_class: &str,
) -> Option<&'static str> {
	if retry_error_class.starts_with("repo_gate_")
		|| error.downcast_ref::<RepoGateFailure>().is_some()
	{
		Some("failed")
	} else {
		None
	}
}

fn write_failure_retry_schedule_marker(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	write_retry_schedule_marker(workflow, issue_run, retry_budget_attempts, "failure")
}

fn write_retry_schedule_marker(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
	retry_kind: &str,
) -> Result<()> {
	let retry_attempt = u32::try_from(retry_budget_attempts).unwrap_or(u32::MAX).max(1);
	let delay = retry_delay(RetryKind::Failure, retry_attempt, workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	state::write_run_retry_schedule(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_kind,
		retry_ready_at_unix_epoch,
	)
}
