use std::collections::HashSet;

use crate::{
	config::ServiceConfig,
	orchestrator::{TERMINAL_GUARDED_RUN_STATUS, worktree_mapping_is_stale_terminal_local_residue},
	prelude::Result,
	state::{IssueLease, StateStore, WorktreeMapping},
};

pub(super) fn clear_stale_terminal_local_worktree_mappings(
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
