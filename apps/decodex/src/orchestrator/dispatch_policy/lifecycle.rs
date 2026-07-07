mod cleanup;

pub(crate) use cleanup::{
	cleanup_completed_post_review_lane, cleanup_terminal_worktree, cleanup_worktree_mapping,
	clear_worktree_retry_schedule,
};

use crate::{
	orchestrator::{
		dispatch_policy,
		dispatch_policy::{
			IssueDispatchMode, IssueTracker, Result, ServiceConfig, StateStore, TrackerIssue,
			WorkflowDocument,
		},
	},
	tracker,
};

pub(crate) fn clear_recovered_issue_lease(
	project_id: &str,
	issue_id: &str,
	expected_run_id: Option<&str>,
	state_store: &StateStore,
) -> Result<()> {
	let Some(lease) = state_store.lease_for_issue(issue_id)? else {
		return Ok(());
	};

	if lease.project_id() != project_id {
		return Ok(());
	}
	if expected_run_id.is_some_and(|run_id| lease.run_id() != run_id) {
		return Ok(());
	}

	state_store.clear_lease(issue_id)
}

pub(crate) fn is_issue_eligible<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let queue_label = tracker::automation_queue_label(project_id);

	if !dispatch_policy::issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)?
	{
		return Ok(false);
	}
	if dispatch_policy::ordinary_dispatch_blocked_by_retained_review_handoff(
		project_id,
		issue,
		state_store,
	)? {
		return Ok(false);
	}

	Ok(state_store.lease_for_issue(&issue.id)?.is_none())
}

pub(crate) fn todo_blocker_rule_passes(issue: &TrackerIssue, workflow: &WorkflowDocument) -> bool {
	if issue.state.name != "Todo" {
		return true;
	}

	issue.blockers.iter().all(|blocker| state_name_is_terminal(&blocker.state.name, workflow))
}

pub(crate) fn refresh_issue<T>(tracker: &T, issue_id: &str) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	let issue_ids = [issue_id.to_owned()];
	let mut refreshed_issues = tracker.refresh_issues(&issue_ids)?;

	Ok(refreshed_issues.pop())
}

pub(crate) fn is_terminal_issue(issue: &TrackerIssue, workflow: &WorkflowDocument) -> bool {
	state_name_is_terminal(&issue.state.name, workflow)
}

pub(crate) fn state_name_is_terminal(state_name: &str, workflow: &WorkflowDocument) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}

pub(crate) fn is_issue_in_progress_for_run(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> bool {
	let tracker_policy = workflow.frontmatter().tracker();

	issue.state.name == tracker_policy.in_progress_state()
		&& !issue.has_label(tracker_policy.needs_attention_label())
}

pub(crate) fn is_issue_not_dispatchable_for_run(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> bool {
	let tracker_policy = workflow.frontmatter().tracker();

	issue.has_label(tracker_policy.opt_out_label())
		|| issue.has_label(tracker_policy.needs_attention_label())
		|| (issue.state.name != tracker_policy.in_progress_state()
			&& !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name))
}

pub(crate) fn is_issue_not_dispatchable_for_current_dispatch<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	match dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			Ok(!dispatch_policy::issue_passes_review_repair_dispatch_policy(
				tracker, issue, project, workflow,
			)?)
		},
		IssueDispatchMode::Normal
		| IssueDispatchMode::Program
		| IssueDispatchMode::Retry
		| IssueDispatchMode::Closeout => Ok(is_issue_not_dispatchable_for_run(issue, workflow)),
	}
}

pub(crate) fn mark_run_attempt_if_active(
	state_store: &StateStore,
	run_id: &str,
	reconciled_status: &str,
) -> Result<()> {
	let Some(run_attempt) = state_store.run_attempt(run_id)? else {
		return Ok(());
	};

	if matches!(run_attempt.status(), "starting" | "running") {
		state_store.update_run_status(run_id, reconciled_status)?;
	}

	Ok(())
}
