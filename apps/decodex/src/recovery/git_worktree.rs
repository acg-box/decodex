//! Git worktree inspection helpers for recovery validation.

mod command;
mod lineage;
mod paths;
mod status;

#[cfg(test)] pub(in crate::recovery) use self::status::worktree_blocking_status_lines;
pub(in crate::recovery) use self::{
	lineage::{ReviewHandoffLineage, worktree_head_descends_from_review_handoff},
	paths::{git_toplevel_path, repository_relative_path, worktree_checkout_branch_name},
	status::{
		worktree_has_tracked_changes_for_recovery,
		worktree_head_has_unmerged_commits_against_remote_default, worktree_head_oid,
		worktree_is_clean,
	},
};
