mod authority;
mod closeout;
mod commands;
mod commit_guard;
mod context;
mod git;
mod landing;
mod model;
mod recovery;

pub(crate) use self::{
	commands::{run_commit, run_land},
	model::{ManualCommitRequest, ManualLandRequest},
};

#[cfg(test)] use std::path::Path;

use self::{
	authority::resolve_land_authority,
	closeout::{ensure_manual_land_left_no_merged_worktree_cleanup_debt, prepare_closeout},
	git::{
		current_branch_name, current_branch_name_if_attached, ensure_clean_worktree,
		paths_match_for_manual_commit_guard, run_git_capture,
	},
	model::{
		LandExecutionMode, MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT, MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS,
		MANUAL_LAND_MERGEABILITY_RETRY_DELAY, ManualAuthority, ManualCommitActiveLaneBlocker,
		ManualLandCloseoutMarkerRecord, ManualLandContext, ManualLandLedgerContext,
		ManualLandRecoveryOutcome, PreparedCloseout,
	},
};
#[cfg(test)]
use self::{
	authority::{infer_issue_identifier_from_worktree_root, looks_like_issue_identifier},
	closeout::{
		apply_closeout, cleanup_manual_land_lane_checkout, clear_manual_closeout_issue_scope,
		clear_manual_closeout_runtime_state, ensure_manual_closeout_issue_scope,
		manual_land_closeout_matches, read_manual_land_closeout_marker,
		write_manual_land_cleanup_complete_event, write_manual_land_closeout_marker,
	},
	commit_guard::manual_commit_active_lane_blocker,
	context::{
		ensure_cli_repo_context, prepare_configured_manual_land_context,
		prepare_unregistered_manual_land_context, read_manual_land_handoff,
		resolve_manual_config_path, resolve_pr_url,
	},
	recovery::ensure_already_merged_manual_land_recovery_ready,
};
#[cfg(test)] use crate::github::RepositoryContext;
#[cfg(test)] use crate::prelude::Result;
#[cfg(test)] use crate::pull_request::PullRequestLandingState;

#[cfg(test)]
fn resolve_authority(
	config_path: Option<&Path>,
	explicit: Option<&str>,
	manual_authority: bool,
	worktree_root: &Path,
) -> Result<ManualAuthority> {
	authority::resolve_authority(config_path, explicit, manual_authority, worktree_root)
}

#[cfg(test)]
fn ensure_manual_land_checkout_is_managed_lane(
	repo_root: &Path,
	worktree_root: &Path,
	identifier: &str,
) -> Result<()> {
	closeout::ensure_manual_land_checkout_is_managed_lane(repo_root, worktree_root, identifier)
}

#[cfg(test)]
fn execute_land_merge(
	context: &ManualLandContext,
	current_head: &str,
	landed_change_record: &str,
	execution_mode: LandExecutionMode,
) -> Result<String> {
	landing::execute_land_merge(context, current_head, landed_change_record, execution_mode)
}

#[cfg(test)]
fn load_authoritative_landed_change_record(
	context: &ManualLandContext,
	merge_commit: &str,
) -> Result<String> {
	landing::load_authoritative_landed_change_record(context, merge_commit)
}

#[cfg(test)]
fn validate_landing_state(
	landing_state: &PullRequestLandingState,
	pr_url: &str,
	expected_base_branch: &str,
	current_branch: &str,
	current_head: &str,
) -> Result<LandExecutionMode> {
	landing::validate_landing_state(
		landing_state,
		pr_url,
		expected_base_branch,
		current_branch,
		current_head,
	)
}

#[cfg(test)]
fn finalize_already_merged_manual_land_recovery(
	context: &ManualLandContext,
	request: &ManualLandRequest,
) -> Result<Option<ManualLandRecoveryOutcome>> {
	recovery::finalize_already_merged_manual_land_recovery(context, request)
}

#[cfg(test)] mod tests;
