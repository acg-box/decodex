mod retry;
mod writeback;

pub(crate) use retry::{
	ensure_automation_activity_label, retry_budget_attempts_for_current_failure,
};

use crate::orchestrator::execution_failure::{
	self, FailureHandlingContext, IssueRunPlan, IssueTracker, LoopGuardrailRecoveryDecision,
	ManualAttentionRequested, Report, Result, ReviewPolicyStopReason, ReviewPolicyStopRequested,
	ServiceConfig, StateStore, WorkflowDocument, disposition,
};

pub(crate) fn handle_failure<T>(
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
	let retry_budget_attempts =
		retry::retry_budget_attempts_for_current_failure(state_store, issue_run)?;
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

	let loop_guardrail_stop =
		retry::retryable_failure_loop_guardrail_stop_unless_terminal_attention(
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
			LoopGuardrailRecoveryDecision::Start(recovery) =>
				writeback::apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				writeback::apply_loop_guardrail_failure_writeback(
					&failure_context,
					loop_guardrail_stop,
				),
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
			LoopGuardrailRecoveryDecision::Start(recovery) =>
				writeback::apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				writeback::apply_loop_guardrail_failure_writeback(
					&failure_context,
					loop_guardrail_stop,
				),
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

	writeback::apply_terminal_attention_failure_writeback(
		&failure_context,
		manual_attention_requested,
		terminal_error,
	)
}
