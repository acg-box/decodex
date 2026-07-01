use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
	config::{self, validation},
	prelude::{Result, eyre},
};

/// Optional service-level path overrides.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPathsConfig {
	repo_root: Option<PathBuf>,
	worktree_root: Option<PathBuf>,
}
impl ProjectPathsConfig {
	pub(super) fn validate(&self) -> Result<()> {
		if self.repo_root.is_none() {
			eyre::bail!("`paths.repo_root` is required for every Decodex project config.");
		}

		if let Some(repo_root) = self.repo_root.as_deref() {
			validation::validate_nonempty_path("paths.repo_root", repo_root)?;
		}
		if let Some(worktree_root) = self.worktree_root.as_deref() {
			validation::validate_nonempty_path("paths.worktree_root", worktree_root)?;
		}

		Ok(())
	}

	pub(super) fn resolve_repo_root(&self, config_dir: &Path) -> Result<PathBuf> {
		let Some(path) = self.repo_root.as_deref() else {
			eyre::bail!("`paths.repo_root` is required for every Decodex project config.");
		};
		let repo_root = config::resolve_relative_path(config_dir, path);
		let repo_root = config::canonicalize_path_best_effort(&repo_root);

		validation::validate_nonempty_path("paths.repo_root", &repo_root)?;

		Ok(repo_root)
	}

	pub(super) fn resolve_worktree_root(&self, repo_root: &Path) -> Result<PathBuf> {
		let worktree_root = self.worktree_root.as_deref().map_or_else(
			|| repo_root.join(".worktrees"),
			|path| config::resolve_relative_path(repo_root, path),
		);

		validation::validate_nonempty_path("paths.worktree_root", &worktree_root)?;

		Ok(worktree_root)
	}
}
