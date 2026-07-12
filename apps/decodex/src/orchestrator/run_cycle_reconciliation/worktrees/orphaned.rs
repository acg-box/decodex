use std::{collections::HashMap, time::Duration};

use crate::{
	orchestrator::{
		self, IssueTracker, RunAttempt, RunLeaseDisposition, RunLeaseReconciliation,
		run_cycle_reconciliation::ProjectStateReconciliationContext,
	},
	prelude::Result,
	state::{self, IssueLease, StateStore, WorktreeMapping},
	tracker::TrackerIssue,
};

pub(in crate::orchestrator::run_cycle_reconciliation) fn reconcile_orphaned_active_worktree_runs<
	T,
>(
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

	let Some(run_attempt) =
		context.state_store.latest_run_attempt_for_lane(context.project.service_id(), &issue.id)?
	else {
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
