use std::path::Path;

use crate::{
	prelude::Result,
	recovery::identifiers,
	state::{ProjectRunStatus, StateStore},
};

pub(super) fn inspect_ghost_lane_worktree(
	worktree_root: &Path,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut retained_worktree_present = false;
	let mut mapping_checked = false;

	if let Some(worktree_path) = run.worktree_path() {
		mapping_checked = true;

		if worktree_path.exists() {
			retained_worktree_present = true;
		} else {
			evidence.push(String::from("worktree_mapping_path_missing"));
		}
	}
	if let Some(mapping) = state_store.worktree_for_issue(run.issue_id())? {
		mapping_checked = true;

		if mapping.worktree_path().exists() {
			retained_worktree_present = true;
		} else {
			evidence.push(String::from("worktree_mapping_path_missing"));
		}
	}

	for selector in
		identifiers::ghost_lane_worktree_selectors(run, issue_identifier, requested_selector)
	{
		if worktree_root.join(&selector).exists() {
			retained_worktree_present = true;
		}
	}

	if retained_worktree_present {
		blockers.push(String::from("retained_worktree_present"));
	} else {
		if !mapping_checked {
			evidence.push(String::from("worktree_mapping_missing"));
		}

		evidence.push(String::from("worktree_missing"));
	}

	Ok(())
}
