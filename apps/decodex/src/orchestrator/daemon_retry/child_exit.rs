mod active;
mod lane;
mod plan;
mod queue;

use crate::orchestrator::daemon_retry::{
	self, CONTINUATION_PENDING_RUN_STATUS, ChildExitPhaseGoalRecovery, ChildExitRetryContext,
	ChildExitRetrySchedule, ChildRunRef, ExitStatus, IssueDispatchMode, IssueTracker, Result,
	RetryEntryLifecycle, RetryEntryRetentionDecision, RetryKind,
};

pub(crate) fn schedule_retry_after_child_exit<T>(
	mut context: ChildExitRetryContext<'_, T>,
	child: ChildRunRef<'_>,
	#[cfg(test)] _retry_project_slug: &str,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	exit_status: ExitStatus,
) -> Result<()>
where
	T: IssueTracker,
{
	let Some(run_attempt) =
		active::active_child_exit_run_attempt(&mut context, child, exit_status)?
	else {
		return Ok(());
	};
	let issue_id = run_attempt.issue_id();
	let Some(issue) = active::refreshed_child_exit_issue(&mut context, issue_id)? else {
		return Ok(());
	};
	let continuation_pending =
		exit_status.success() && run_attempt.status() == CONTINUATION_PENDING_RUN_STATUS;

	if !exit_status.success() && run_attempt.status() != "failed" {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;

		return Ok(());
	}

	let retention_decision = daemon_retry::child_exit_retry_retention_decision(
		&context,
		&issue,
		initial_issue_state,
		RetryEntryLifecycle::for_dispatch_mode(dispatch_mode),
		continuation_pending,
	)?;

	if retention_decision == RetryEntryRetentionDecision::Drop {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;

		return Ok(());
	}

	let recovered_phase_goal_continuation = match daemon_retry::recover_child_exit_phase_goal(
		&mut context,
		&issue,
		child,
		issue_id,
		initial_issue_state,
		dispatch_mode,
		exit_status.success(),
	)? {
		ChildExitPhaseGoalRecovery::None => None,
		ChildExitPhaseGoalRecovery::Continuation(recovery) => Some(recovery),
		ChildExitPhaseGoalRecovery::Terminalized => return Ok(()),
	};
	let (kind, attempt, continuation_initial_issue_state) = if continuation_pending {
		plan::continuation_child_exit_retry_plan(&run_attempt, initial_issue_state)
	} else if recovered_phase_goal_continuation.is_some() {
		context
			.state_store
			.update_run_status(run_attempt.run_id(), CONTINUATION_PENDING_RUN_STATUS)?;

		plan::continuation_child_exit_retry_plan(&run_attempt, initial_issue_state)
	} else if exit_status.success() {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;

		return Ok(());
	} else {
		let retry_budget_attempts =
			daemon_retry::child_exit_retry_budget_attempt_count(&context, &issue, child)?;
		let retry_budget_limit =
			daemon_retry::child_exit_retry_budget_limit(&context, &issue, child)?;

		if retry_budget_attempts >= retry_budget_limit {
			return daemon_retry::terminalize_exhausted_child_exit_retry(
				context,
				issue,
				child,
				initial_issue_state,
				dispatch_mode,
				retry_budget_attempts,
			);
		}

		(RetryKind::Failure, retry_budget_attempts, None)
	};

	if !lane::child_exit_lane_decision_permits_retry(
		&mut context,
		&issue,
		&run_attempt,
		dispatch_mode,
		kind,
	)? {
		return Ok(());
	}

	queue::queue_child_exit_retry(
		context.retry_queue,
		context.state_store,
		context.workflow,
		ChildExitRetrySchedule {
			project_id: context.project.service_id(),
			issue_id,
			run_id: run_attempt.run_id(),
			attempt_number: run_attempt.attempt_number(),
			continuation_initial_issue_state,
			dispatch_mode,
			kind,
			attempt,
		},
	)
}
