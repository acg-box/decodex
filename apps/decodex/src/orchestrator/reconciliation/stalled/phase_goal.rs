use crate::orchestrator::reconciliation::{
	self, CONTINUATION_PENDING_RUN_STATUS, IssueRunPlan, OffsetDateTime,
	RUN_OPERATION_RECONCILIATION, Result, RetryKind, ServiceConfig, StateStore, TrackerIssue,
	WorkflowDocument, stalled::markers,
};

pub(super) fn try_recover_stalled_retained_phase_goal(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
	issue_run: &IssueRunPlan,
) -> Result<bool> {
	markers::write_reconciliation_operation_marker_best_effort(
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
