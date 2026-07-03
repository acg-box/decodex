use std::{fs, path::Path};

use crate::{
	prelude::{Result, eyre},
	workflow::WorkflowWorkspaceHooks,
	worktree::{WorktreeManager, git, hooks},
};

impl WorktreeManager {
	pub(crate) fn remove_worktree_path(&self, path: &Path) -> Result<bool> {
		self.remove_worktree_path_internal(path, None)
	}

	pub(crate) fn remove_worktree_path_with_hooks(
		&self,
		issue_identifier: &str,
		branch_name: &str,
		path: &Path,
		hooks: &WorkflowWorkspaceHooks,
	) -> Result<bool> {
		self.remove_worktree_path_internal(path, Some((issue_identifier, branch_name, hooks)))
	}

	pub(super) fn remove_worktree_path_internal(
		&self,
		path: &Path,
		hooks: Option<(&str, &str, &WorkflowWorkspaceHooks)>,
	) -> Result<bool> {
		if !path.try_exists().map_err(|error| {
			eyre::eyre!("Failed to inspect worktree path `{}`: {error}", path.display())
		})? {
			return Ok(false);
		}

		let worktree_root = fs::canonicalize(&self.worktree_root)?;
		let canonical_path = fs::canonicalize(path)?;

		if !canonical_path.starts_with(&worktree_root) || canonical_path == worktree_root {
			eyre::bail!(
				"Refusing to remove worktree `{}` outside worktree_root `{}`.",
				path.display(),
				self.worktree_root.display()
			);
		}
		if hooks::remove_orphan_marker_directory_if_safe(&canonical_path)? {
			return Ok(true);
		}

		self.validate_worktree_boundary(&canonical_path)?;

		if let Some((issue_identifier, branch_name, hooks)) = hooks {
			self.run_workspace_hook_phase(
				"before_remove",
				issue_identifier,
				branch_name,
				&canonical_path,
				hooks.before_remove_commands(),
				hooks.timeout_seconds(),
			)?;
		}

		git::run_git(
			&self.repo_root,
			[
				"worktree",
				"remove",
				"--force",
				canonical_path.as_os_str().to_str().ok_or_else(|| {
					eyre::eyre!("Worktree path `{}` is not valid UTF-8.", canonical_path.display())
				})?,
			],
			"remove the linked worktree",
		)?;

		Ok(true)
	}
}
