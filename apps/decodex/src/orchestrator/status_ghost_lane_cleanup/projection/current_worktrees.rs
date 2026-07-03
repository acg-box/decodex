use std::{collections::BTreeSet, path::Path};

use crate::{
	config::ServiceConfig,
	orchestrator::{self, OperatorStatusSnapshot, OperatorWorktreeStatus, status_run_projection},
	prelude::Result,
	state::StateStore,
};

pub(super) fn operator_snapshot_current_worktree_keys(
	project: &ServiceConfig,
	snapshot: &OperatorStatusSnapshot,
) -> BTreeSet<String> {
	snapshot
		.worktrees
		.iter()
		.filter(|worktree| operator_worktree_status_path_exists(project, worktree))
		.map(|worktree| {
			orchestrator::operator_issue_attention_key(
				&worktree.issue_id,
				worktree.issue_identifier.as_deref(),
			)
		})
		.collect()
}

pub(super) fn ghost_lane_current_worktree_keys(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<BTreeSet<String>> {
	let mut keys = BTreeSet::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if !mapping.worktree_path().exists() {
			continue;
		}

		let issue_identifier =
			status_run_projection::issue_identifier_in_text(mapping.branch_name()).or_else(|| {
				status_run_projection::issue_identifier_in_text(
					&mapping.worktree_path().display().to_string(),
				)
			});

		keys.insert(orchestrator::operator_issue_attention_key(
			mapping.issue_id(),
			issue_identifier.as_deref(),
		));
	}
	for issue_identifier in orchestrator::recoverable_worktree_identifiers(project.worktree_root())?
	{
		if project.worktree_root().join(&issue_identifier).exists() {
			keys.insert(orchestrator::operator_issue_attention_key(
				&issue_identifier,
				Some(&issue_identifier),
			));
		}
	}

	Ok(keys)
}

fn operator_worktree_status_path_exists(
	project: &ServiceConfig,
	worktree: &OperatorWorktreeStatus,
) -> bool {
	let path = Path::new(&worktree.worktree_path);

	if path.is_absolute() { path.exists() } else { project.repo_root().join(path).exists() }
}
