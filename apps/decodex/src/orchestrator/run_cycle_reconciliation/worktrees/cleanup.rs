use std::{
	collections::{HashMap, HashSet},
	path::Path,
};

use crate::{
	orchestrator::{
		self, IssueTracker, run_cycle_reconciliation::ProjectStateReconciliationContext,
	},
	prelude::Result,
	state::{StateStore, WorktreeMapping},
	lane_authority::LaneClaim,
	tracker::TrackerIssue,
};

pub(in crate::orchestrator::run_cycle_reconciliation) fn cleanup_missing_orphaned_project_worktree_mappings<
	T,
>(
	context: &ProjectStateReconciliationContext<'_, T>,
	claims: &[LaneClaim],
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<()>
where
	T: IssueTracker,
{
	let leased_issue_ids =
		claims.iter().map(|claim| claim.id().tracker_issue_id()).collect::<HashSet<_>>();

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

		let service_owned = orchestrator::issue_has_service_ownership(
			context.tracker,
			issue,
			context.project.service_id(),
		)?;
		let has_active_claim = context
			.state_store
			.issue_has_active_shared_claim(context.project.service_id(), &issue.id)?;
		let has_running_attempt = issue_has_running_attempt(
			context.state_store,
			context.project.service_id(),
			&issue.id,
		)?;
		let has_review_lifecycle = context
			.state_store
			.review_lifecycle_record(
				context.project.service_id(),
				mapping.issue_id(),
				mapping.branch_name(),
			)?
			.is_some();

		if service_owned
			|| issue.has_label(context.workflow.frontmatter().tracker().needs_attention_label())
			|| has_active_claim
			|| has_running_attempt
			|| has_review_lifecycle
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

fn issue_has_running_attempt(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
) -> Result<bool> {
	Ok(state_store
		.latest_run_attempt_for_lane(project_id, issue_id)?
		.is_some_and(|attempt| matches!(attempt.status(), "starting" | "running")))
}
