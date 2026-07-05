mod gitdir;
mod linked;
mod search;
mod worktree_roots;

use std::path::{Path, PathBuf};

use crate::{
	config::{git_paths, path_resolution},
	prelude::Result,
};

pub(crate) fn shared_repo_root_for_checkout(
	cwd: &Path,
	worktree_root: Option<&Path>,
) -> Result<Option<PathBuf>> {
	let git_dir = git_paths::git_absolute_rev_parse(cwd, "git-dir")?
		.map(|path| path_resolution::canonicalize_path_best_effort(&path));
	let common_dir = git_paths::git_absolute_rev_parse(cwd, "git-common-dir")?
		.map(|path| path_resolution::canonicalize_path_best_effort(&path));
	let prefers_shared_repo_root = git_dir.is_some() && git_dir != common_dir;

	if prefers_shared_repo_root {
		return linked::shared_repo_root_for_linked_worktree(
			cwd,
			worktree_root,
			common_dir.as_deref(),
		);
	}

	Ok(None)
}
