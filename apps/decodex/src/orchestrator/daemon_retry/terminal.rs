use crate::orchestrator::daemon_retry::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_RETRY_KIND, ChildExitRetryContext,
	ChildRunRef, IssueDispatchMode, IssueRunPlan, IssueTracker, Report, Result,
	RetainedPartialProgress, TERMINAL_GUARDED_RUN_STATUS, TerminalFailureWritebackRuntime,
	TrackerIssue, WorktreeManager, WorktreeSpec, state,
};

pub(crate) fn child_exit_retry_budget_attempt_count<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
) -> Result<u32>
where
	T: IssueTracker,
{
	let state_attempts = context.state_store.retry_budget_attempt_count(&issue.id)?.max(1);
	let worktree = child_exit_worktree_spec(context, issue)?;
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(u32::try_from(state_attempts).unwrap_or(u32::MAX).max(1));
	};
	let marker_attempts = state::read_run_retry_budget_attempt_count(&worktree.path)?.unwrap_or(0);
	let marker_is_current_child =
		marker.run_id() == child.run_id && marker.attempt_number() == child.attempt_number;
	let marker_attempt_is_local = context.state_store.run_attempt(marker.run_id())?.is_some();
	let retry_budget_attempts =
		if marker_attempts > 0 && !marker_is_current_child && !marker_attempt_is_local {
			marker_attempts.saturating_add(state_attempts)
		} else {
			marker_attempts.max(state_attempts)
		};

	Ok(u32::try_from(retry_budget_attempts).unwrap_or(u32::MAX).max(1))
}

pub(crate) fn child_exit_retry_budget_limit<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
) -> Result<u32>
where
	T: IssueTracker,
{
	let max_attempts = context.workflow.frontmatter().execution().max_attempts();
	let worktree = child_exit_worktree_spec(context, issue)?;
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(max_attempts);
	};

	if marker.run_id() == child.run_id
		&& marker.attempt_number() == child.attempt_number
		&& marker.retry_kind() == Some(ARCHITECTURE_RECOVERY_RETRY_KIND)
	{
		return Ok(
			max_attempts.saturating_add(u32::try_from(ARCHITECTURE_RECOVERY_BUDGET).unwrap_or(0))
		);
	}

	Ok(max_attempts)
}

pub(crate) fn terminalize_exhausted_child_exit_retry<T>(
	context: ChildExitRetryContext<'_, T>,
	issue: TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	retry_budget_attempts: u32,
) -> Result<()>
where
	T: IssueTracker,
{
	apply_child_exit_terminal_failure_writeback(
		&context,
		&issue,
		child,
		initial_issue_state,
		dispatch_mode,
		i64::from(retry_budget_attempts),
	)?;

	daemon_retry::clear_retry_schedule_and_release(
		context.retry_queue,
		context.state_store,
		child.issue_id,
	)?;

	Ok(())
}

pub(crate) fn child_exit_worktree_spec<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<WorktreeSpec>
where
	T: IssueTracker,
{
	if let Some(mapping) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(WorktreeSpec {
			branch_name: mapping.branch_name().to_owned(),
			issue_identifier: issue.identifier.clone(),
			path: mapping.worktree_path().to_path_buf(),
			reused_existing: true,
		});
	}

	let worktree_manager = WorktreeManager::new(
		context.project.service_id(),
		context.project.repo_root(),
		context.project.worktree_root(),
	);

	Ok(worktree_manager.plan_for_issue(&issue.identifier))
}

fn apply_child_exit_terminal_failure_writeback<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	retry_budget_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let worktree = child_exit_worktree_spec(context, issue)?;
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
	let worktree_path = daemon_retry::relative_worktree_path(context.project, &issue_run.worktree);
	let error = if daemon_retry::worktree_has_tracked_changes(&issue_run.worktree.path) {
		Report::new(RetainedPartialProgress {
			issue_identifier: issue.identifier.clone(),
			run_id: child.run_id.to_owned(),
			worktree_path: worktree_path.clone(),
			source_error_class: None,
		})
	} else {
		Report::msg(format!(
			"Daemon child `{}` for issue `{}` exited unsuccessfully after exhausting retry budget.",
			child.run_id, issue.identifier
		))
	};
	let privacy_classifier =
		daemon_retry::configured_public_projection_privacy_classifier(context.project)?;
	let outcome = daemon_retry::apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		&issue_run,
		&worktree_path,
		false,
		&error,
	)?;

	if outcome.retry_guarded_by_state {
		daemon_retry::write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		context.state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	daemon_retry::write_retry_budget_marker(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = issue.id,
		issue = issue.identifier,
		run_id = child.run_id,
		attempt = child.attempt_number,
		retry_budget_attempt = retry_budget_attempts,
		branch = issue_run.worktree.branch_name,
		worktree_path = %worktree_path,
		error_class = outcome.error_class,
		"Daemon child failed and now requires operator attention."
	);

	Ok(())
}
