use std::collections::HashSet;

use crate::{
	config::ServiceConfig,
	orchestrator,
	prelude::Result,
	state::{StateStore, WORKTREE_PROVENANCE_RUNTIME_RECORDED, WorktreeMapping},
};

pub(crate) fn worktree_mapping_is_stale_terminal_local_residue(
	project: &ServiceConfig,
	state_store: &StateStore,
	mapping: &WorktreeMapping,
	active_issue_ids: &HashSet<String>,
) -> Result<bool> {
	if active_issue_ids.contains(mapping.issue_id())
		|| !orchestrator::looks_like_tracker_issue_identifier_key(mapping.issue_id())
		|| mapping.provenance().source() != WORKTREE_PROVENANCE_RUNTIME_RECORDED
	{
		return Ok(false);
	}
	if state_store.issue_has_active_shared_claim(project.service_id(), mapping.issue_id())? {
		return Ok(false);
	}
	if state_store.issue_has_review_lifecycle_record(project.service_id(), mapping.issue_id())?
		|| state_store
			.issue_has_review_policy_checkpoint(project.service_id(), mapping.issue_id())?
	{
		return Ok(false);
	}
	if mapping.worktree_path().try_exists()? {
		return Ok(false);
	}

	let Some(attempt) = state_store.latest_run_attempt_for_issue(mapping.issue_id())? else {
		return Ok(false);
	};

	Ok(orchestrator::local_run_attempt_status_is_terminal(attempt.status()))
}
