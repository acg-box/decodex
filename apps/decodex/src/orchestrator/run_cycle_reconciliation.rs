mod leases;
mod terminal_cleanup;
mod worktrees;

pub(crate) use self::{
	leases::{retained_closeout_lease_has_fresh_activity, terminal_issue_keeps_retained_closeout},
	terminal_cleanup::{
		local_run_attempt_status_is_terminal, looks_like_tracker_issue_identifier_key,
	},
};

use std::collections::{HashMap, HashSet};

use time::OffsetDateTime;

use crate::{
	orchestrator::{
		IssueTracker, ServiceConfig, StateStore, TERMINAL_GUARDED_RUN_STATUS, TrackerIssue,
		WorkflowDocument, WorktreeManager, WorktreeMapping, closeout_dispatch_block_reason,
		is_terminal_issue, issue_passes_closeout_dispatch_policy, mark_run_attempt_if_active,
		retained_review_handoff_matches_run, worktree_activity_marker_is_fresh,
		worktree_mapping_is_stale_terminal_local_residue,
	},
	prelude::Result,
	state::IssueLease,
	tracker,
};
use leases::clear_terminal_lane_labels_once;

pub(in crate::orchestrator::run_cycle_reconciliation) struct ProjectStateReconciliationContext<
	'a,
	T,
> {
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

	terminal_cleanup::clear_stale_terminal_local_worktree_mappings(
		project,
		state_store,
		&leases,
		&mut worktrees,
	)?;

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

	leases::reconcile_active_project_leases(
		&reconciliation_context,
		&leases,
		&issues_by_id,
		now_unix_epoch,
		&mut cleared_terminal_lane_issue_ids,
	)?;
	worktrees::cleanup_missing_orphaned_project_worktree_mappings(
		&reconciliation_context,
		&leases,
		&worktrees,
		&issues_by_id,
	)?;
	worktrees::reconcile_orphaned_active_worktree_runs(
		&reconciliation_context,
		&leases,
		&worktrees,
		&issues_by_id,
		now_unix_epoch,
	)?;
	worktrees::cleanup_terminal_project_worktrees(
		&reconciliation_context,
		&worktrees,
		&issues_by_id,
		&mut cleared_terminal_lane_issue_ids,
	)?;

	Ok(())
}
