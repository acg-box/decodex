use std::path::Path;

use crate::{
	recovery::stale_active_worktree,
	state::{self, RunActivityMarker, StateStore, WorktreeMapping},
};

pub(super) fn record_stale_active_run_lease_evidence(
	run_lease: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if run_lease {
		blockers.push(String::from("run_lease_present"));
	} else {
		evidence.push(String::from("run_lease_missing"));
	}
}

pub(super) fn read_stale_active_worktree_mapping(
	state_store: &StateStore,
	issue_keys: &[String],
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Option<WorktreeMapping> {
	match stale_active_worktree::stale_active_worktree_mapping_for_keys(state_store, issue_keys) {
		Ok(mapping) => mapping,
		Err(error) => {
			blockers.push(String::from("worktree_mapping_ambiguous"));
			evidence.push(format!("worktree_mapping_error:{}", error));

			None
		},
	}
}

pub(super) fn read_stale_active_activity_marker(
	worktree_path: &Path,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Option<RunActivityMarker> {
	match state::read_run_activity_marker_snapshot(worktree_path) {
		Ok(marker) => marker,
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			None
		},
	}
}
