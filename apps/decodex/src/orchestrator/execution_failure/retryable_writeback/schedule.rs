use crate::{
	orchestrator::execution_failure::{
		self, AppServerCapabilityPreflightFailure, AppServerPhaseGoalFailure, IssueRunPlan,
		OffsetDateTime, RepoGateFailure, Report, Result, RetryKind, StalledRunNeedsAttention,
		WorkflowDocument,
	},
	state,
};

pub(crate) fn write_retry_schedule_marker_for_runtime_retry(
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
	let delay = execution_failure::retry_delay(RetryKind::Failure, retry_attempt, workflow);
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
