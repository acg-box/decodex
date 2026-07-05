use std::path::Path;

use crate::{
	recovery::{
		git_worktree, process_liveness::StaleActiveProcessLiveness, stale_active_worktree::marker,
	},
	state::{self, RunActivityMarker, WorktreeMapping},
};

pub(in crate::recovery) fn inspect_stale_active_worktree(
	worktree_path: &Path,
	mapping: Option<&WorktreeMapping>,
	activity_marker: Option<&RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	if mapping.is_none() {
		evidence.push(String::from("worktree_mapping_missing"));
	}

	match worktree_path.try_exists() {
		Ok(false) => {
			evidence.push(String::from("worktree_missing"));

			return String::from("missing");
		},
		Ok(true) => {},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}

	marker::inspect_stale_active_activity_marker(
		activity_marker,
		marker_liveness,
		evidence,
		blockers,
	);

	if let Some(status) = inspect_non_git_worktree(worktree_path, evidence, blockers) {
		return status;
	}
	if let Some(status) = inspect_git_worktree_progress(worktree_path, evidence, blockers) {
		return status;
	}

	evidence.push(String::from("worktree_clean"));

	inspect_worktree_head_reachability(worktree_path, evidence, blockers)
}

fn inspect_non_git_worktree(
	worktree_path: &Path,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Option<String> {
	match worktree_path.join(".git").try_exists() {
		Ok(false) =>
			match state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path) {
				Ok(true) => {
					evidence.push(String::from("worktree_non_git_marker_directory"));

					Some(String::from("non_git_marker_directory"))
				},
				Ok(false) => {
					blockers.push(String::from("non_git_worktree_files_present"));

					Some(String::from("non_git_files_present"))
				},
				Err(error) => {
					blockers.push(String::from("worktree_tracked_changes_unknown"));
					evidence.push(format!("worktree_status_error:{}", error));

					Some(String::from("tracked_changes_unknown"))
				},
			},
		Ok(true) => None,
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			Some(String::from("tracked_changes_unknown"))
		},
	}
}

fn inspect_git_worktree_progress(
	worktree_path: &Path,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Option<String> {
	match git_worktree::worktree_has_tracked_changes_for_recovery(worktree_path) {
		Ok(true) => {
			blockers.push(String::from("worktree_tracked_changes_present"));

			Some(String::from("tracked_changes_present"))
		},
		Ok(false) => None,
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			Some(String::from("tracked_changes_unknown"))
		},
	}
}

fn inspect_worktree_head_reachability(
	worktree_path: &Path,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	match git_worktree::worktree_head_has_unmerged_commits_against_remote_default(worktree_path) {
		Ok(Some(true)) => {
			blockers.push(String::from("worktree_unmerged_commits_present"));

			String::from("unmerged_commits_present")
		},
		Ok(Some(false)) => {
			evidence.push(String::from("worktree_head_reachable_from_default_branch"));

			String::from("clean")
		},
		Ok(None) => {
			blockers.push(String::from("worktree_default_branch_unavailable"));
			evidence.push(String::from("worktree_default_branch_unavailable"));

			String::from("default_branch_unavailable")
		},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_head_status_error:{}", error));

			String::from("tracked_changes_unknown")
		},
	}
}
