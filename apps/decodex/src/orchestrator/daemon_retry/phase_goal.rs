use crate::orchestrator::daemon_retry::{
	self, ChildExitPhaseGoalRecovery, ChildExitRetryContext, ChildRunRef, IssueDispatchMode,
	IssueRunPlan, IssueTracker, Result, TrackerIssue,
};

pub(crate) fn recover_child_exit_phase_goal<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	issue_id: &str,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	exit_success: bool,
) -> Result<ChildExitPhaseGoalRecovery>
where
	T: IssueTracker,
{
	if exit_success {
		return Ok(ChildExitPhaseGoalRecovery::None);
	}

	let recovery = maybe_recover_child_exit_phase_goal_continuation(
		context,
		issue,
		child,
		initial_issue_state,
		dispatch_mode,
	)?;

	if matches!(recovery, ChildExitPhaseGoalRecovery::Terminalized) {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;
	}

	Ok(recovery)
}

fn maybe_recover_child_exit_phase_goal_continuation<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
) -> Result<ChildExitPhaseGoalRecovery>
where
	T: IssueTracker,
{
	let worktree = daemon_retry::child_exit_worktree_spec(context, issue)?;
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: initial_issue_state.to_owned(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode,
		attempt_number: child.attempt_number,
		run_id: child.run_id.to_owned(),
		retry_budget_base: 0,
	};
	let recovery = match daemon_retry::recover_phase_goal_continuation(
		context.project,
		context.workflow,
		context.state_store,
		&issue_run,
		"child_exit_failed",
		Some("child_exit_failed"),
	) {
		Ok(recovery) => recovery,
		Err(error) if daemon_retry::run_failure_requires_terminal_attention(&error) => {
			daemon_retry::handle_failure(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&issue_run,
				&error,
			)?;

			return Ok(ChildExitPhaseGoalRecovery::Terminalized);
		},
		Err(error) => return Err(error),
	};

	if let Some(recovery) = &recovery {
		tracing::warn!(
			project_id = context.project.service_id(),
			issue_id = issue.id,
			issue = issue.identifier,
			run_id = child.run_id,
			attempt = child.attempt_number,
			source_phase = recovery.source_phase.as_str(),
			next_phase = recovery.next_phase.as_str(),
			"Recovered phase goal after child exit failure; scheduling continuation."
		);
	}

	Ok(recovery.map_or(ChildExitPhaseGoalRecovery::None, |recovery| {
		ChildExitPhaseGoalRecovery::Continuation(recovery)
	}))
}
