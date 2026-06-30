use std::{
	collections::{HashMap, HashSet},
	path::Path,
	time::Duration,
};

use time::OffsetDateTime;

use super::{
	IssueTracker, RunAttempt, RunLeaseDisposition, RunLeaseReconciliation, ServiceConfig,
	StateStore, TERMINAL_GUARDED_RUN_STATUS, TrackerIssue, WorkflowDocument, WorktreeManager,
	WorktreeMapping, apply_run_lease_reconciliation, cleanup_worktree_mapping,
	closeout_dispatch_block_reason, is_issue_in_progress_for_run, is_terminal_issue,
	issue_has_service_ownership, issue_passes_closeout_dispatch_policy, mark_run_attempt_if_active,
	marker_process_is_alive, observed_idle_duration, retained_review_handoff_matches_run,
	stalled_idle_duration, worktree_activity_marker_is_fresh, worktree_has_tracked_changes,
	worktree_mapping_is_stale_terminal_local_residue,
};
use crate::{
	prelude::Result,
	state::{self, IssueLease},
	tracker,
};

struct ProjectStateReconciliationContext<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	worktree_manager: &'a WorktreeManager,
}

pub(crate) fn reconcile_project_state<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
) -> Result<()>
where
	T: IssueTracker,
{
	let leases = state_store.list_leases(project.service_id())?;
	let mut worktrees = state_store.list_worktrees(project.service_id())?;

	if leases.is_empty() && worktrees.is_empty() {
		return Ok(());
	}

	clear_stale_terminal_local_worktree_mappings(project, state_store, &leases, &mut worktrees)?;

	if leases.is_empty() && worktrees.is_empty() {
		return Ok(());
	}

	let mut issue_ids = HashSet::new();

	for lease in &leases {
		issue_ids.insert(lease.issue_id().to_owned());
	}
	for mapping in &worktrees {
		issue_ids.insert(mapping.issue_id().to_owned());
	}

	let refreshed_issues = tracker.refresh_issues(&issue_ids.into_iter().collect::<Vec<_>>())?;
	let issues_by_id = refreshed_issues
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect::<HashMap<_, _>>();
	let reconciliation_context = ProjectStateReconciliationContext {
		tracker,
		project,
		workflow,
		state_store,
		worktree_manager,
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut cleared_terminal_lane_issue_ids = HashSet::new();

	reconcile_active_project_leases(
		&reconciliation_context,
		&leases,
		&issues_by_id,
		now_unix_epoch,
		&mut cleared_terminal_lane_issue_ids,
	)?;
	cleanup_missing_orphaned_project_worktree_mappings(
		&reconciliation_context,
		&leases,
		&worktrees,
		&issues_by_id,
	)?;
	reconcile_orphaned_active_worktree_runs(
		&reconciliation_context,
		&leases,
		&worktrees,
		&issues_by_id,
		now_unix_epoch,
	)?;
	cleanup_terminal_project_worktrees(
		&reconciliation_context,
		&worktrees,
		&issues_by_id,
		&mut cleared_terminal_lane_issue_ids,
	)?;

	Ok(())
}

fn clear_stale_terminal_local_worktree_mappings(
	project: &ServiceConfig,
	state_store: &StateStore,
	leases: &[IssueLease],
	worktrees: &mut Vec<WorktreeMapping>,
) -> Result<()> {
	let active_issue_ids =
		leases.iter().map(|lease| lease.issue_id().to_owned()).collect::<HashSet<_>>();
	let mut cleared_issue_ids = Vec::new();

	for mapping in worktrees.iter() {
		if !worktree_mapping_is_stale_terminal_local_residue(
			project,
			state_store,
			mapping,
			&active_issue_ids,
		)? {
			continue;
		}

		state_store.clear_worktree(mapping.issue_id())?;

		tracing::info!(
			project_id = project.service_id(),
			issue_id = mapping.issue_id(),
			provenance_source = mapping.provenance().source(),
			"Cleared stale terminal local worktree mapping before tracker refresh."
		);

		cleared_issue_ids.push(mapping.issue_id().to_owned());
	}

	if !cleared_issue_ids.is_empty() {
		worktrees.retain(|mapping| {
			!cleared_issue_ids.iter().any(|issue_id| issue_id == mapping.issue_id())
		});
	}

	Ok(())
}

pub(crate) fn looks_like_tracker_issue_identifier_key(value: &str) -> bool {
	let Some((prefix, number)) = value.rsplit_once('-') else {
		return false;
	};

	!prefix.is_empty()
		&& !number.is_empty()
		&& prefix
			.chars()
			.all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
		&& number.chars().all(|character| character.is_ascii_digit())
}

pub(crate) fn local_run_attempt_status_is_terminal(status: &str) -> bool {
	matches!(
		status,
		"succeeded" | "failed" | "interrupted" | "terminated" | TERMINAL_GUARDED_RUN_STATUS
	)
}

fn reconcile_active_project_leases<T>(
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
		mark_run_attempt_if_active(context.state_store, lease.run_id(), "succeeded")?;

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
	mark_run_attempt_if_active(context.state_store, lease.run_id(), "interrupted")?;

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
		Some(issue) if is_terminal_issue(issue, context.workflow) => "terminated",
		Some(_) | None => "interrupted",
	};

	if let Some(issue) = issues_by_id.get(lease.issue_id())
		&& is_terminal_issue(issue, context.workflow)
	{
		clear_terminal_lane_labels_once(
			context.tracker,
			context.project,
			issue,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	mark_run_attempt_if_active(context.state_store, lease.run_id(), reconciled_status)?;

	context.state_store.clear_lease(lease.issue_id())
}

fn cleanup_missing_orphaned_project_worktree_mappings<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<()>
where
	T: IssueTracker,
{
	let leased_issue_ids = leases.iter().map(IssueLease::issue_id).collect::<HashSet<_>>();

	for mapping in worktrees {
		if leased_issue_ids.contains(mapping.issue_id())
			|| mapping.provenance().is_legacy_unknown()
			|| !worktree_mapping_path_is_missing(mapping.worktree_path())
		{
			continue;
		}

		let Some(issue) = issues_by_id.get(mapping.issue_id()) else {
			continue;
		};

		if issue_has_service_ownership(context.tracker, issue, context.project.service_id())?
			|| issue.has_label(context.workflow.frontmatter().tracker().needs_attention_label())
			|| context
				.state_store
				.issue_has_active_shared_claim(context.project.service_id(), &issue.id)?
			|| issue_has_running_attempt(context.state_store, &issue.id)?
			|| context
				.state_store
				.review_handoff_marker(
					context.project.service_id(),
					mapping.issue_id(),
					mapping.branch_name(),
				)?
				.is_some()
		{
			continue;
		}

		context.state_store.clear_worktree(mapping.issue_id())?;
	}

	Ok(())
}

fn worktree_mapping_path_is_missing(worktree_path: &Path) -> bool {
	matches!(worktree_path.try_exists(), Ok(false))
}

fn issue_has_running_attempt(state_store: &StateStore, issue_id: &str) -> Result<bool> {
	Ok(state_store
		.latest_run_attempt_for_issue(issue_id)?
		.is_some_and(|attempt| matches!(attempt.status(), "starting" | "running")))
}

fn reconcile_orphaned_active_worktree_runs<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut orphaned_actions = Vec::new();

	for mapping in worktrees {
		if leases.iter().any(|lease| lease.issue_id() == mapping.issue_id()) {
			continue;
		}

		let Some(issue) = issues_by_id.get(mapping.issue_id()) else {
			continue;
		};
		let Some(action) = inspect_orphaned_active_worktree_reconciliation(
			context,
			issue,
			mapping,
			now_unix_epoch,
		)?
		else {
			continue;
		};

		orphaned_actions.push(action);
	}

	apply_run_lease_reconciliation(
		context.tracker,
		context.project,
		context.state_store,
		context.worktree_manager,
		orphaned_actions,
	)
}

fn cleanup_terminal_project_worktrees<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	for mapping in worktrees {
		if let Some(issue) = issues_by_id.get(mapping.issue_id())
			&& is_terminal_issue(issue, context.workflow)
			&& !terminal_issue_keeps_retained_closeout(
				context.tracker,
				issue,
				context.project,
				context.workflow,
				context.state_store,
			)? {
			clear_terminal_lane_labels_once(
				context.tracker,
				context.project,
				issue,
				cleared_terminal_lane_issue_ids,
			)?;
			cleanup_worktree_mapping(
				context.state_store,
				context.worktree_manager,
				context.workflow,
				&issue.identifier,
				mapping,
			)?;
		}
	}

	Ok(())
}

fn inspect_orphaned_active_worktree_reconciliation<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	issue: &TrackerIssue,
	worktree_mapping: &WorktreeMapping,
	now_unix_epoch: i64,
) -> Result<Option<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	let has_service_ownership =
		issue_has_service_ownership(context.tracker, issue, context.project.service_id())?;
	let needs_attention =
		issue.has_label(context.workflow.frontmatter().tracker().needs_attention_label());

	if !has_service_ownership && !needs_attention {
		return Ok(None);
	}

	let Some(run_attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? else {
		return Ok(None);
	};
	let Some(idle_for) = orphaned_run_lease_idle_duration(
		context.state_store,
		&run_attempt,
		worktree_mapping,
		now_unix_epoch,
	)?
	else {
		return Ok(None);
	};
	let disposition = if needs_attention {
		RunLeaseDisposition::StalledAlreadyNeedsAttention { idle_for }
	} else if is_issue_in_progress_for_run(issue, context.workflow)
		&& worktree_has_tracked_changes(worktree_mapping.worktree_path())
	{
		RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
	} else if is_issue_in_progress_for_run(issue, context.workflow) {
		RunLeaseDisposition::Stalled { idle_for }
	} else {
		return Ok(None);
	};

	Ok(Some(RunLeaseReconciliation {
		issue: issue.clone(),
		run_attempt,
		worktree_mapping: Some(worktree_mapping.clone()),
		disposition,
		workflow: context.workflow.clone(),
	}))
}

fn orphaned_run_lease_idle_duration(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: &WorktreeMapping,
	now_unix_epoch: i64,
) -> Result<Option<Duration>> {
	if !matches!(run_attempt.status(), "starting" | "running") {
		return Ok(None);
	}

	let marker = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
		.filter(|marker| {
			marker.run_id() == run_attempt.run_id()
				&& marker.attempt_number() == run_attempt.attempt_number()
		});

	if let Some(marker) = marker.as_ref()
		&& marker.process_id().is_some()
	{
		if marker_process_is_alive(marker) {
			return Ok(None);
		}

		return Ok(Some(
			marker
				.last_activity_unix_epoch()
				.and_then(|last_activity| observed_idle_duration(last_activity, now_unix_epoch))
				.unwrap_or(Duration::ZERO),
		));
	}

	stalled_idle_duration(state_store, run_attempt, Some(worktree_mapping), now_unix_epoch)
}

fn clear_terminal_lane_labels_once<T>(
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

fn retained_review_lease_matches_run(state_store: &StateStore, lease: &IssueLease) -> Result<bool> {
	let Some(run_attempt) = state_store.run_attempt(lease.run_id())? else {
		return Ok(false);
	};
	let worktree_mapping = state_store.worktree_for_issue(lease.issue_id())?;

	retained_review_handoff_matches_run(state_store, &run_attempt, worktree_mapping.as_ref())
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
	if !is_terminal_issue(issue, workflow) {
		return Ok(false);
	}

	Ok(issue_passes_closeout_dispatch_policy(tracker, issue, project, workflow, state_store)?
		|| closeout_dispatch_block_reason(tracker, issue, project, workflow, state_store)?
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
		&& worktree_activity_marker_is_fresh(&marker, now_unix_epoch))
}
