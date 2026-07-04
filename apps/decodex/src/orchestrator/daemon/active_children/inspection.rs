use crate::orchestrator::daemon::{
	self, ActiveWorkflowOverride, CurrentChildRunContext, IssueDispatchMode, IssueTracker,
	OffsetDateTime, Result, RunLeaseDisposition, RunLeaseReconciliation, ServiceConfig, StateStore,
	WorkflowDocument,
};

pub(crate) fn inspect_current_daemon_child_reconciliation<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	child_context: CurrentChildRunContext<'_>,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	inspect_current_daemon_child_reconciliation_at(
		tracker,
		project,
		workflow,
		state_store,
		child_context,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
}

pub(crate) fn inspect_current_daemon_child_reconciliation_at<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	child_context: CurrentChildRunContext<'_>,
	now_unix_epoch: i64,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	let child = child_context.child;
	let Some(issue) = daemon::refresh_issue(tracker, child.issue_id)? else {
		return Ok(Vec::new());
	};
	let Some(run_attempt) = state_store.run_attempt(child.run_id)? else {
		return Ok(Vec::new());
	};
	let worktree_mapping = state_store.worktree_for_issue(&issue.id)?;

	if let Some(disposition) = daemon::superseded_run_disposition(state_store, &run_attempt)? {
		return Ok(vec![RunLeaseReconciliation {
			issue: issue.clone(),
			run_attempt,
			worktree_mapping,
			disposition,
			workflow: workflow.clone(),
		}]);
	}

	let action_workflow = daemon::run_lease_reconciliation_workflow(
		workflow,
		Some(ActiveWorkflowOverride { child, workflow: child_context.workflow }),
		&issue,
		&run_attempt,
	);
	let retained_closeout = daemon::terminal_issue_keeps_retained_closeout(
		tracker,
		&issue,
		project,
		action_workflow,
		state_store,
	)?;
	let completed_closeout_child =
		matches!(child_context.dispatch_mode, IssueDispatchMode::Closeout)
			&& daemon::is_terminal_issue(&issue, action_workflow);
	let disposition = if !retained_closeout
		&& !completed_closeout_child
		&& daemon::is_terminal_issue(&issue, action_workflow)
	{
		Some(RunLeaseDisposition::Terminal)
	} else if !retained_closeout
		&& !completed_closeout_child
		&& daemon::is_issue_not_dispatchable_for_current_dispatch(
			tracker,
			&issue,
			project,
			action_workflow,
			child_context.dispatch_mode,
		)? {
		Some(RunLeaseDisposition::NotDispatchable)
	} else if let Some(idle_for) = daemon::stalled_idle_duration(
		state_store,
		&run_attempt,
		worktree_mapping.as_ref(),
		now_unix_epoch,
	)? {
		if daemon::retained_review_handoff_matches_run(
			state_store,
			&run_attempt,
			worktree_mapping.as_ref(),
		)? {
			Some(RunLeaseDisposition::RetainedReviewComplete)
		} else if daemon::stalled_run_has_retained_partial_progress(worktree_mapping.as_ref()) {
			Some(RunLeaseDisposition::StalledRetainedPartialProgress { idle_for })
		} else {
			Some(RunLeaseDisposition::Stalled { idle_for })
		}
	} else {
		None
	};

	Ok(disposition.map_or_else(Vec::new, |disposition| {
		vec![RunLeaseReconciliation {
			issue: issue.clone(),
			run_attempt,
			worktree_mapping,
			disposition,
			workflow: action_workflow.clone(),
		}]
	}))
}
