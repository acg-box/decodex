use crate::{
	orchestrator::execution_failure::{
		self, ARCHITECTURE_RECOVERY_RETRY_KIND, ArchitectureRecoveryStart, FailureHandlingContext,
		HarnessOutcomeKind, IssueTracker, LoopGuardrailStopRequested, OffsetDateTime, Report,
		Result, RetryComment, RetryKind, TERMINAL_GUARDED_RUN_STATUS,
		TerminalFailureWritebackRuntime,
	},
	state, tracker,
};

pub(crate) fn apply_terminal_attention_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	manual_attention_requested: bool,
	terminal_error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	let privacy_classifier =
		execution_failure::configured_public_projection_privacy_classifier(context.project)?;
	let outcome = execution_failure::apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		context.issue_run,
		context.worktree_path,
		manual_attention_requested,
		terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		execution_failure::write_terminal_guard_marker(
			&context.issue_run.worktree.path,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
		)?;

		context
			.state_store
			.update_run_status(&context.issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	execution_failure::write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		error_class = outcome.error_class,
		"Run failed and now requires operator attention."
	);

	Ok(())
}

pub(crate) fn apply_architecture_recovery_retry_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	recovery: ArchitectureRecoveryStart,
	max_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let retry_attempt = u32::try_from(context.retry_budget_attempts).unwrap_or(u32::MAX).max(1);
	let delay = execution_failure::retry_delay(RetryKind::Failure, retry_attempt, context.workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);
	let recovery_max_attempts =
		max_attempts.saturating_add(i64::try_from(recovery.max_attempts).unwrap_or(0));

	state::write_run_retry_schedule(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		ARCHITECTURE_RECOVERY_RETRY_KIND,
		retry_ready_at_unix_epoch,
	)?;
	execution_failure::write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		recovery_attempt = recovery.attempt_number,
		max_recovery_attempts = recovery.max_attempts,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		"Loop guardrail started autonomous architecture recovery."
	);

	tracker::create_public_comment(
		context.tracker,
		&context.issue_run.issue.id,
		&execution_failure::format_retry_comment(RetryComment {
			run_id: &context.issue_run.run_id,
			attempt_number: context.issue_run.attempt_number,
			retry_budget_attempt_number: context.retry_budget_attempts,
			max_attempts: recovery_max_attempts,
			worktree_path: context.worktree_path.to_owned(),
			branch_name: &context.issue_run.worktree.branch_name,
			error_class: "architecture_recovery_started",
			next_action: execution_failure::architecture_recovery_retry_next_action(
				recovery.policy_decision,
			),
		}),
	)?;
	execution_failure::record_harness_outcome_best_effort(
		context.state_store,
		context.project.service_id(),
		context.issue_run,
		HarnessOutcomeKind::RetryableFailure,
		Some("architecture_recovery_started"),
		Some("architecture_recovery_started"),
		None,
	);

	Ok(())
}

pub(crate) fn apply_loop_guardrail_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	loop_guardrail_stop: LoopGuardrailStopRequested,
) -> Result<()>
where
	T: IssueTracker,
{
	let terminal_error = Report::new(loop_guardrail_stop);
	let privacy_classifier =
		execution_failure::configured_public_projection_privacy_classifier(context.project)?;
	let outcome = execution_failure::apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		context.issue_run,
		context.worktree_path,
		false,
		&terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		execution_failure::write_terminal_guard_marker(
			&context.issue_run.worktree.path,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
		)?;

		context
			.state_store
			.update_run_status(&context.issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	execution_failure::write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = context.worktree_path,
		error_class = outcome.error_class,
		"Run stopped by loop guardrail."
	);

	Ok(())
}
