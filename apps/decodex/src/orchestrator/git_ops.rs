mod branch;
mod command;
mod diagnostic;
mod failure;
mod paths;
mod rewrite;
mod runner;
mod selection;

pub(crate) use self::branch::{LocalRefDeleteReadback, delete_local_branch_at_oid};
#[cfg(test)]
pub(crate) use self::command::{
	repo_gate_shell_from_env, run_repo_gate_cleanliness_check_with_git,
};
pub(crate) use self::{
	branch::{delete_local_branch_if_present, detach_worktree_head_from_branch_if_checked_out},
	command::repo_gate_changed_tracked_files,
	diagnostic::{RepoGateFailureDiagnostic, repo_gate_output_text},
	failure::{RepoGateFailure, RepoGateFailureDisposition, RepoGateFailureKind},
	paths::{relative_worktree_path, relative_worktree_path_for_path},
	rewrite::{RepoGateCommandOutcome, RepoGateTrackedRewriteDecision},
	runner::{
		run_canonicalize_commands, run_repo_gate_commands,
		run_repo_gate_commands_with_owned_rewrites,
	},
	selection::select_repo_gate_for_worktree,
};
