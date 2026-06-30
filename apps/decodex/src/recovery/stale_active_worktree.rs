//! Worktree mapping and status inspection for stale-active recovery.

use std::path::Path;

use crate::{
	prelude::{Result, eyre},
	state::{self, StateStore, WorktreeMapping},
};

use super::{
	git_worktree::{
		worktree_has_tracked_changes_for_recovery,
		worktree_head_has_unmerged_commits_against_remote_default,
	},
	process_liveness::{StaleActiveProcessLiveness, stale_active_marker_thread_active},
};

pub(super) fn stale_active_worktree_mapping_for_keys(
	state_store: &StateStore,
	issue_keys: &[String],
) -> Result<Option<WorktreeMapping>> {
	let mut mapping = None;

	for issue_key in issue_keys {
		let Some(candidate) = state_store.worktree_for_issue(issue_key)? else {
			continue;
		};
		if let Some(existing) = mapping.as_ref() {
			if stale_active_worktree_mappings_conflict(existing, &candidate) {
				eyre::bail!(
					"conflicting retained worktree mappings for stale active issue keys `{}`",
					issue_keys.join(", ")
				);
			}
		} else {
			mapping = Some(candidate);
		}
	}

	Ok(mapping)
}

pub(super) fn inspect_stale_active_worktree(
	worktree_path: &Path,
	mapping: Option<&WorktreeMapping>,
	marker: Option<&state::RunActivityMarker>,
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
	inspect_stale_active_activity_marker(marker, marker_liveness, evidence, blockers);
	match worktree_path.join(".git").try_exists() {
		Ok(false) => {
			match state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path) {
				Ok(true) => {
					evidence.push(String::from("worktree_non_git_marker_directory"));

					return String::from("non_git_marker_directory");
				},
				Ok(false) => {
					blockers.push(String::from("non_git_worktree_files_present"));

					return String::from("non_git_files_present");
				},
				Err(error) => {
					blockers.push(String::from("worktree_tracked_changes_unknown"));
					evidence.push(format!("worktree_status_error:{}", error));

					return String::from("tracked_changes_unknown");
				},
			}
		},
		Ok(true) => {},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}
	match worktree_has_tracked_changes_for_recovery(worktree_path) {
		Ok(true) => {
			blockers.push(String::from("worktree_tracked_changes_present"));

			return String::from("tracked_changes_present");
		},
		Ok(false) => {},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}
	evidence.push(String::from("worktree_clean"));
	match worktree_head_has_unmerged_commits_against_remote_default(worktree_path) {
		Ok(Some(true)) => {
			blockers.push(String::from("worktree_unmerged_commits_present"));

			return String::from("unmerged_commits_present");
		},
		Ok(Some(false)) => {
			evidence.push(String::from("worktree_head_reachable_from_default_branch"));
		},
		Ok(None) => {
			blockers.push(String::from("worktree_default_branch_unavailable"));
			evidence.push(String::from("worktree_default_branch_unavailable"));

			return String::from("default_branch_unavailable");
		},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_head_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}

	String::from("clean")
}

fn stale_active_worktree_mappings_conflict(
	left: &WorktreeMapping,
	right: &WorktreeMapping,
) -> bool {
	left.branch_name() != right.branch_name() || left.worktree_path() != right.worktree_path()
}

fn inspect_stale_active_activity_marker(
	marker: Option<&state::RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if let Some(marker) = marker {
		match marker_liveness {
			StaleActiveProcessLiveness::Alive => blockers.push(String::from("process_alive")),
			StaleActiveProcessLiveness::NotAlive =>
				evidence.push(String::from("process_not_alive")),
			StaleActiveProcessLiveness::Unknown =>
				blockers.push(String::from("process_liveness_unknown")),
		}
		if marker.last_progress_unix_epoch().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_progress_present"));
			} else {
				blockers.push(String::from("activity_marker_progress_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_progress_missing"));
		}
		if marker.event_count() > 0 || marker.last_event_type().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_protocol_event_marker_present"));
			} else {
				blockers.push(String::from("protocol_event_marker_present"));
			}
		} else {
			evidence.push(String::from("protocol_event_marker_missing"));
		}
		if marker.last_protocol_activity_unix_epoch().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_protocol_activity_present"));
			} else {
				blockers.push(String::from("activity_marker_protocol_activity_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_protocol_activity_missing"));
		}
		if marker.child_agent_activity().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_child_agent_activity_present"));
			} else {
				blockers.push(String::from("activity_marker_child_agent_activity_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_child_agent_activity_missing"));
		}
		if marker.protocol_activity().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence
					.push(String::from("stale_activity_marker_protocol_activity_summary_present"));
			} else {
				blockers.push(String::from("activity_marker_protocol_activity_summary_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_protocol_activity_summary_missing"));
		}
		if stale_active_marker_thread_active(marker) {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_thread_active"));
			} else {
				blockers.push(String::from("activity_marker_thread_active"));
			}
		} else {
			evidence.push(String::from("activity_marker_thread_inactive"));
		}
	} else {
		evidence.push(String::from("activity_marker_missing"));
	}
}
