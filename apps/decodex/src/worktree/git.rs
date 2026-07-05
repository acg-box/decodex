pub(in crate::worktree) mod remote;

mod branch;
mod command;
mod registry;

pub(super) use self::{
	branch::{configured_branch_owner, sanitize_branch_component},
	command::{git_stdout, run_git},
	registry::{resolve_source_repo_git_common_dir, worktree_is_registered},
	remote::{fetch_remote_branch_if_present, normalize_origin_remote_for_worktrees},
};
