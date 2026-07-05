mod cleanup;
mod creation;
mod git;
mod hook_runner;
mod hooks;
mod removal;

#[allow(unused_imports)] pub(crate) use cleanup::MergedWorktreeCleanliness;
pub(crate) use cleanup::{
	MergedWorktreeCleanupDebt, infer_default_branch_name, merged_worktree_cleanup_debts,
};

use std::path::PathBuf;

#[cfg(test)] use git::remote::is_relative_filesystem_remote;
#[cfg(test)] use hooks::workspace_hook_shell_from_env;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorktreeSpec {
	pub(crate) branch_name: String,
	pub(crate) issue_identifier: String,
	pub(crate) path: PathBuf,
	pub(crate) reused_existing: bool,
}

pub(crate) struct WorktreeManager {
	repo_root: PathBuf,
	worktree_root: PathBuf,
	project_id: String,
}
impl WorktreeManager {
	pub(crate) fn new(
		project_id: impl Into<String>,
		repo_root: impl Into<PathBuf>,
		worktree_root: impl Into<PathBuf>,
	) -> Self {
		Self {
			repo_root: repo_root.into(),
			worktree_root: worktree_root.into(),
			project_id: project_id.into(),
		}
	}

	pub(crate) fn plan_for_issue(&self, issue_identifier: &str) -> WorktreeSpec {
		let branch_suffix = git::sanitize_branch_component(issue_identifier);
		let branch_owner =
			git::configured_branch_owner(&self.repo_root).unwrap_or_else(|| String::from("x"));
		let branch_name = format!(
			"{}/{}-{}",
			git::sanitize_branch_component(&branch_owner),
			git::sanitize_branch_component(&self.project_id),
			branch_suffix
		);
		let path = self.worktree_root.join(issue_identifier);
		let reused_existing = path.join(".git").exists();

		WorktreeSpec {
			branch_name,
			issue_identifier: issue_identifier.to_owned(),
			path,
			reused_existing,
		}
	}
}

#[cfg(test)] mod tests;
