use std::{
	collections::{HashMap, HashSet},
	path::Path,
	time::Duration,
};

use crate::{
	orchestrator::{
		self, IssueTracker, RunAttempt, RunLeaseDisposition, RunLeaseReconciliation,
		run_cycle_reconciliation::{self, ProjectStateReconciliationContext},
	},
	prelude::Result,
	state::{self, IssueLease, StateStore, WorktreeMapping},
	tracker::TrackerIssue,
};

pub(super) fn cleanup_missing_orphaned_project_worktree_mappings<T>(
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

		if orchestrator::issue_has_service_ownership(
			context.tracker,
			issue,
			context.project.service_id(),
		)? || issue.has_label(context.workflow.frontmatter().tracker().needs_attention_label())
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

pub(super) fn reconcile_orphaned_active_worktree_runs<T>(
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

	orchestrator::apply_run_lease_reconciliation(
		context.tracker,
		context.project,
		context.state_store,
		context.worktree_manager,
		orphaned_actions,
	)
}

pub(super) fn cleanup_terminal_project_worktrees<T>(
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
			&& orchestrator::is_terminal_issue(issue, context.workflow)
			&& !orchestrator::terminal_issue_keeps_retained_closeout(
				context.tracker,
				issue,
				context.project,
				context.workflow,
				context.state_store,
			)? {
			run_cycle_reconciliation::clear_terminal_lane_labels_once(
				context.tracker,
				context.project,
				issue,
				cleared_terminal_lane_issue_ids,
			)?;
			orchestrator::cleanup_worktree_mapping(
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

fn worktree_mapping_path_is_missing(worktree_path: &Path) -> bool {
	matches!(worktree_path.try_exists(), Ok(false))
}

fn issue_has_running_attempt(state_store: &StateStore, issue_id: &str) -> Result<bool> {
	Ok(state_store
		.latest_run_attempt_for_issue(issue_id)?
		.is_some_and(|attempt| matches!(attempt.status(), "starting" | "running")))
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
	let has_service_ownership = orchestrator::issue_has_service_ownership(
		context.tracker,
		issue,
		context.project.service_id(),
	)?;
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
	} else if orchestrator::is_issue_in_progress_for_run(issue, context.workflow)
		&& orchestrator::worktree_has_tracked_changes(worktree_mapping.worktree_path())
	{
		RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
	} else if orchestrator::is_issue_in_progress_for_run(issue, context.workflow) {
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
		if orchestrator::marker_process_is_alive(marker) {
			return Ok(None);
		}

		return Ok(Some(
			marker
				.last_activity_unix_epoch()
				.and_then(|last_activity| {
					orchestrator::observed_idle_duration(last_activity, now_unix_epoch)
				})
				.unwrap_or(Duration::ZERO),
		));
	}

	orchestrator::stalled_idle_duration(
		state_store,
		run_attempt,
		Some(worktree_mapping),
		now_unix_epoch,
	)
}
