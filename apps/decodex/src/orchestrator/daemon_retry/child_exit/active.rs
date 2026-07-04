use crate::orchestrator::{
	RunAttempt, TrackerIssue,
	daemon_retry::{self, ChildExitRetryContext, ChildRunRef, ExitStatus, IssueTracker, Result},
};

pub(crate) fn active_child_exit_run_attempt<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	child: ChildRunRef<'_>,
	exit_status: ExitStatus,
) -> Result<Option<RunAttempt>>
where
	T: IssueTracker,
{
	let Some(run_attempt) =
		daemon_retry::resolve_child_exit_run_attempt(context.state_store, child)?
	else {
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			"Daemon child exited without a matching recorded run attempt; skipping retry scheduling."
		);

		return Ok(None);
	};

	if !exit_status.success() {
		daemon_retry::mark_run_attempt_if_active(
			context.state_store,
			run_attempt.run_id(),
			"failed",
		)?;
	}

	let Some(run_attempt) = context.state_store.run_attempt(run_attempt.run_id())? else {
		return Ok(None);
	};

	if daemon_retry::superseded_run_disposition(context.state_store, &run_attempt)?.is_some() {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			child.issue_id,
		)?;

		return Ok(None);
	}

	Ok(Some(run_attempt))
}

pub(crate) fn refreshed_child_exit_issue<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue_id: &str,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	let issue = daemon_retry::refresh_issue(context.tracker, issue_id)?;

	if issue.is_none() {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;
	}

	Ok(issue)
}
