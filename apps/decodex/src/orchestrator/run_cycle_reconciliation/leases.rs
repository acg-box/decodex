use crate::{
	orchestrator::{
		run_cycle_reconciliation,
		run_cycle_reconciliation::{
			HashMap, HashSet, IssueLease, IssueTracker, ProjectStateReconciliationContext, Result,
			ServiceConfig, StateStore, TrackerIssue, WorkflowDocument, WorktreeManager, tracker,
		},
	},
	state,
};

pub(super) fn reconcile_active_project_leases<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	for lease in leases {
		if reconcile_success_retained_review_lease(context, lease, issues_by_id)? {
			continue;
		}
		if reconcile_terminal_retained_closeout_lease(
			context,
			lease,
			issues_by_id,
			now_unix_epoch,
			cleared_terminal_lane_issue_ids,
		)? {
			continue;
		}

		reconcile_stale_project_lease(
			context,
			lease,
			issues_by_id,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	Ok(())
}

pub(super) fn clear_terminal_lane_labels_once<T>(
	tracker: &T,
	project: &ServiceConfig,
	issue: &TrackerIssue,
	cleared_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	if cleared_issue_ids.insert(issue.id.clone()) {
		tracker::clear_automation_lane_labels(tracker, issue, project.service_id())?;
	}

	Ok(())
}

pub(crate) fn terminal_issue_keeps_retained_closeout<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	if !run_cycle_reconciliation::is_terminal_issue(issue, workflow) {
		return Ok(false);
	}

	Ok(run_cycle_reconciliation::issue_passes_closeout_dispatch_policy(
		tracker,
		issue,
		project,
		workflow,
		state_store,
	)? || run_cycle_reconciliation::closeout_dispatch_block_reason(
		tracker,
		issue,
		project,
		workflow,
		state_store,
	)?
	.is_some())
}

pub(crate) fn retained_closeout_lease_has_fresh_activity(
	lease: &IssueLease,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	now_unix_epoch: i64,
) -> Result<bool> {
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let worktree = worktree_manager.plan_for_issue(&issue.identifier);
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == lease.run_id()
		&& run_cycle_reconciliation::worktree_activity_marker_is_fresh(&marker, now_unix_epoch))
}

fn reconcile_success_retained_review_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<bool>
where
	T: IssueTracker,
{
	if let Some(issue) = issues_by_id.get(lease.issue_id())
		&& issue.state.name == context.workflow.frontmatter().tracker().success_state()
		&& retained_review_lease_matches_run(context.state_store, lease)?
	{
		run_cycle_reconciliation::mark_run_attempt_if_active(
			context.state_store,
			lease.run_id(),
			"succeeded",
		)?;

		context.state_store.clear_lease(lease.issue_id())?;

		return Ok(true);
	}

	Ok(false)
}

fn reconcile_terminal_retained_closeout_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let Some(issue) = issues_by_id.get(lease.issue_id()) else {
		return Ok(false);
	};

	if !terminal_issue_keeps_retained_closeout(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
	)? {
		return Ok(false);
	}
	if retained_closeout_lease_has_fresh_activity(lease, issue, context.project, now_unix_epoch)? {
		return Ok(true);
	}

	clear_terminal_lane_labels_once(
		context.tracker,
		context.project,
		issue,
		cleared_terminal_lane_issue_ids,
	)?;

	run_cycle_reconciliation::mark_run_attempt_if_active(
		context.state_store,
		lease.run_id(),
		"interrupted",
	)?;

	context.state_store.clear_lease(lease.issue_id())?;

	Ok(true)
}

fn reconcile_stale_project_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	let reconciled_status = match issues_by_id.get(lease.issue_id()) {
		Some(issue) if run_cycle_reconciliation::is_terminal_issue(issue, context.workflow) =>
			"terminated",
		Some(_) | None => "interrupted",
	};

	if let Some(issue) = issues_by_id.get(lease.issue_id())
		&& run_cycle_reconciliation::is_terminal_issue(issue, context.workflow)
	{
		clear_terminal_lane_labels_once(
			context.tracker,
			context.project,
			issue,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	run_cycle_reconciliation::mark_run_attempt_if_active(
		context.state_store,
		lease.run_id(),
		reconciled_status,
	)?;

	context.state_store.clear_lease(lease.issue_id())
}

fn retained_review_lease_matches_run(state_store: &StateStore, lease: &IssueLease) -> Result<bool> {
	let Some(run_attempt) = state_store.run_attempt(lease.run_id())? else {
		return Ok(false);
	};
	let worktree_mapping = state_store.worktree_for_issue(lease.issue_id())?;

	run_cycle_reconciliation::retained_review_handoff_matches_run(
		state_store,
		&run_attempt,
		worktree_mapping.as_ref(),
	)
}
