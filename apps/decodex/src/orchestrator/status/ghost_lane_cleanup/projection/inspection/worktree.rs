use std::collections::BTreeSet;

use crate::{
	commit_message,
	config::ServiceConfig,
	orchestrator::{self, OperatorRunStatus},
	prelude::Result,
	state::StateStore,
};

pub(in crate::orchestrator::status::ghost_lane_cleanup::projection::inspection) fn inspect_status_ghost_lane_worktree(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	current_worktree_keys: &BTreeSet<String>,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut retained_worktree_present = false;
	let mut mapping_checked = false;

	if let Some(worktree_path) = run.worktree_path.as_ref() {
		mapping_checked = true;

		if project.repo_root().join(worktree_path).exists() {
			retained_worktree_present = true;
		} else {
			conditions.push(String::from("worktree_mapping_path_missing"));
		}
	}
	if let Some(mapping) = state_store.worktree_for_issue(&run.issue_id)? {
		mapping_checked = true;

		if mapping.worktree_path().exists() {
			retained_worktree_present = true;
		} else {
			conditions.push(String::from("worktree_mapping_path_missing"));
		}
	}

	let selector = orchestrator::operator_run_tracker_issue_identifier_selector(run);

	for candidate in [selector.as_deref(), Some(run.issue_id.as_str())].into_iter().flatten() {
		if commit_message::looks_like_issue_identifier(candidate)
			&& project.worktree_root().join(candidate).exists()
		{
			retained_worktree_present = true;
		}
	}

	let run_issue_key =
		orchestrator::operator_issue_attention_key(&run.issue_id, run.issue_identifier.as_deref());

	if current_worktree_keys.contains(&run_issue_key) {
		retained_worktree_present = true;
	}
	if retained_worktree_present {
		blockers.push(String::from("retained_worktree_present"));
	} else {
		if !mapping_checked {
			conditions.push(String::from("worktree_mapping_missing"));
		}

		conditions.push(String::from("worktree_missing"));
	}

	Ok(())
}
