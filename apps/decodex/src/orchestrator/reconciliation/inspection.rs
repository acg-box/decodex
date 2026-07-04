use crate::orchestrator::{
	self, ActiveWorkflowOverride, IssueTracker, OffsetDateTime, Result, RunAttempt,
	RunLeaseDisposition, RunLeaseReconciliation, ServiceConfig, StateStore, TrackerIssue,
	WorkflowDocument,
	reconciliation::{idle, stalled},
};
#[cfg(test)] use crate::orchestrator::{HashMap, dispatch_policy::lifecycle};

#[cfg(test)]
pub(crate) fn inspect_run_lease_reconciliation_at<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	active_workflow_override: Option<ActiveWorkflowOverride<'_>>,
	now_unix_epoch: i64,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	let leases = state_store.list_leases(project.service_id())?;

	if leases.is_empty() {
		return Ok(Vec::new());
	}

	let issue_ids = leases.iter().map(|lease| lease.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = tracker.refresh_issues(&issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();
	let mut actions = Vec::new();

	for lease in leases {
		let Some(issue) = issues_by_id.get(lease.issue_id()).cloned() else {
			continue;
		};
		let Some(run_attempt) = state_store.run_attempt(lease.run_id())? else {
			continue;
		};
		let worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
		let action_workflow = run_lease_reconciliation_workflow(
			workflow,
			active_workflow_override,
			&issue,
			&run_attempt,
		);
		let retained_closeout = orchestrator::terminal_issue_keeps_retained_closeout(
			tracker,
			&issue,
			project,
			action_workflow,
			state_store,
		)?;
		let disposition = if let Some(disposition) =
			stalled::superseded_run_disposition(state_store, &run_attempt)?
		{
			Some(disposition)
		} else if !retained_closeout && orchestrator::is_terminal_issue(&issue, action_workflow) {
			Some(RunLeaseDisposition::Terminal)
		} else if !retained_closeout
			&& lifecycle::is_issue_not_dispatchable_for_run(&issue, action_workflow)
		{
			Some(RunLeaseDisposition::NotDispatchable)
		} else if let Some(idle_for) = idle::stalled_idle_duration(
			state_store,
			&run_attempt,
			worktree_mapping.as_ref(),
			now_unix_epoch,
		)? {
			if stalled::retained_review_handoff_matches_run(
				state_store,
				&run_attempt,
				worktree_mapping.as_ref(),
			)? {
				Some(RunLeaseDisposition::RetainedReviewComplete)
			} else if stalled::stalled_run_has_retained_partial_progress(worktree_mapping.as_ref())
			{
				Some(RunLeaseDisposition::StalledRetainedPartialProgress { idle_for })
			} else {
				Some(RunLeaseDisposition::Stalled { idle_for })
			}
		} else {
			None
		};

		if let Some(disposition) = disposition {
			actions.push(RunLeaseReconciliation {
				issue: issue.clone(),
				run_attempt,
				worktree_mapping,
				disposition,
				workflow: action_workflow.clone(),
			});
		}
	}

	Ok(actions)
}

pub(crate) fn inspect_exited_daemon_child_reconciliation_at<T>(
	tracker: &T,
	_project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	now_unix_epoch: i64,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	let Some(issue) = orchestrator::refresh_issue(tracker, issue_id)? else {
		return Ok(Vec::new());
	};
	let Some(run_attempt) = state_store.run_attempt(run_id)? else {
		return Ok(Vec::new());
	};
	let worktree_mapping = state_store.worktree_for_issue(issue_id)?;

	if let Some(disposition) = stalled::superseded_run_disposition(state_store, &run_attempt)? {
		return Ok(vec![RunLeaseReconciliation {
			issue,
			run_attempt,
			worktree_mapping,
			disposition,
			workflow: workflow.clone(),
		}]);
	}

	if run_attempt.status() != "failed"
		|| !orchestrator::is_issue_in_progress_for_run(&issue, workflow)
	{
		return Ok(Vec::new());
	}

	let Some(idle_for) = idle::stalled_protocol_idle_duration(
		state_store,
		&run_attempt,
		worktree_mapping.as_ref(),
		now_unix_epoch,
	)?
	else {
		return Ok(Vec::new());
	};
	let disposition =
		if stalled::stalled_run_has_retained_partial_progress(worktree_mapping.as_ref()) {
			RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
		} else {
			RunLeaseDisposition::Stalled { idle_for }
		};

	Ok(vec![RunLeaseReconciliation {
		issue,
		run_attempt,
		worktree_mapping,
		disposition,
		workflow: workflow.clone(),
	}])
}

pub(crate) fn inspect_exited_daemon_child_reconciliation<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
) -> Result<Vec<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	inspect_exited_daemon_child_reconciliation_at(
		tracker,
		project,
		workflow,
		state_store,
		issue_id,
		run_id,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
}

pub(crate) fn run_lease_reconciliation_workflow<'a>(
	current_workflow: &'a WorkflowDocument,
	active_workflow_override: Option<ActiveWorkflowOverride<'a>>,
	issue: &TrackerIssue,
	run_attempt: &RunAttempt,
) -> &'a WorkflowDocument {
	match active_workflow_override {
		Some(override_context)
			if override_context.child.issue_id == issue.id
				&& override_context.child.run_id == run_attempt.run_id() =>
			override_context.workflow,
		_ => current_workflow,
	}
}
