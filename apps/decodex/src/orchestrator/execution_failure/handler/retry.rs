use std::slice;

use crate::{
	orchestrator::execution_failure::{
		self, IssueRunPlan, IssueTracker, LoopGuardrailStopRequested, Report, Result,
		ServiceConfig, StateStore, TrackerIssue,
	},
	tracker,
};

pub(crate) fn loop_guardrail_stop_unless_terminal_attention(
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

pub(crate) fn retry_budget_attempts_for_current_failure(
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

pub(crate) fn ensure_automation_activity_label<T>(
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
