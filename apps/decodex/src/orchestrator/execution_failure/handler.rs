use std::slice;

use crate::orchestrator::execution_failure::{
	self, ARCHITECTURE_RECOVERY_RETRY_KIND, ArchitectureRecoveryStart, FailureHandlingContext,
	HarnessOutcomeKind, IssueRunPlan, IssueTracker, LoopGuardrailRecoveryDecision,
	LoopGuardrailStopRequested, ManualAttentionRequested, OffsetDateTime, Report, Result,
	RetryComment, RetryKind, ReviewPolicyStopReason, ReviewPolicyStopRequested, ServiceConfig,
	StateStore, TERMINAL_GUARDED_RUN_STATUS, TerminalFailureWritebackRuntime, TrackerIssue,
	WorkflowDocument, disposition,
};
use crate::{state, tracker};

pub(in crate::orchestrator) fn handle_failure<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	let max_attempts = i64::from(workflow.frontmatter().execution().max_attempts());
	let manual_attention_requested = error.downcast_ref::<ManualAttentionRequested>().is_some();
	let requires_terminal_attention =
		execution_failure::run_failure_requires_terminal_attention(error);
	let worktree_path = execution_failure::relative_worktree_path(project, &issue_run.worktree);
	let retry_budget_attempts = retry_budget_attempts_for_current_failure(state_store, issue_run)?;
	let failure_context = FailureHandlingContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		worktree_path: &worktree_path,
		retry_budget_attempts,
	};

	if execution_failure::handle_review_handoff_failure_drift(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		error,
		&worktree_path,
	)? {
		return Ok(());
	}

	let loop_guardrail_stop = retryable_failure_loop_guardrail_stop_unless_terminal_attention(
		project,
		state_store,
		issue_run,
		error,
		requires_terminal_attention,
	)?;
	let retained_partial_progress =
		disposition::retained_partial_progress_error(error, issue_run, &worktree_path);

	if let Some(review_policy_stop) = error.downcast_ref::<ReviewPolicyStopRequested>()
		&& review_policy_stop.reason == ReviewPolicyStopReason::Exhausted
	{
		return match execution_failure::loop_guardrail_architecture_recovery_decision(
			project,
			state_store,
			issue_run,
			execution_failure::loop_guardrail_stop_from_review_policy(review_policy_stop),
			error,
		)? {
			LoopGuardrailRecoveryDecision::Start(recovery) => {
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				)
			},
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) => {
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop)
			},
		};
	}
	if let Some(loop_guardrail_stop) = loop_guardrail_stop {
		return match execution_failure::loop_guardrail_architecture_recovery_decision(
			project,
			state_store,
			issue_run,
			loop_guardrail_stop,
			error,
		)? {
			LoopGuardrailRecoveryDecision::Start(recovery) => {
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				)
			},
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) => {
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop)
			},
		};
	}

	if !requires_terminal_attention && retry_budget_attempts < max_attempts {
		return execution_failure::apply_retryable_failure_writeback(
			&failure_context,
			error,
			max_attempts,
		);
	}

	let terminal_error = retained_partial_progress.as_ref().unwrap_or(error);

	apply_terminal_attention_failure_writeback(
		&failure_context,
		manual_attention_requested,
		terminal_error,
	)
}

pub(in crate::orchestrator) fn retryable_failure_loop_guardrail_stop_unless_terminal_attention(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
	requires_terminal_attention: bool,
) -> Result<Option<LoopGuardrailStopRequested>> {
	if requires_terminal_attention {
		Ok(None)
	} else {
		execution_failure::retryable_failure_loop_guardrail_stop(
			project,
			state_store,
			issue_run,
			error,
		)
	}
}

pub(in crate::orchestrator) fn retry_budget_attempts_for_current_failure(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<i64> {
	let state_attempts = state_store.retry_budget_attempt_count(&issue_run.issue.id)?;
	let current_attempt_counts =
		state_store.run_attempt(&issue_run.run_id)?.is_some_and(|attempt| {
			attempt.issue_id() == issue_run.issue.id
				&& matches!(attempt.status(), "failed" | "interrupted" | "terminal_guarded")
		});
	let previous_state_attempts = state_attempts.saturating_sub(i64::from(current_attempt_counts));

	Ok(issue_run.retry_budget_base.max(previous_state_attempts) + i64::from(current_attempt_counts))
}

pub(in crate::orchestrator) fn ensure_automation_activity_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	present: bool,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&issue.id))?;
	let current_issue = refreshed_issues.pop().unwrap_or_else(|| issue.clone());
	let active_label = tracker::automation_active_label(service_id);

	tracker::set_issue_label_presence(tracker, &current_issue, &active_label, present)?;

	Ok(())
}

fn apply_terminal_attention_failure_writeback<T>(
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

fn apply_architecture_recovery_retry_writeback<T>(
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

fn apply_loop_guardrail_failure_writeback<T>(
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
