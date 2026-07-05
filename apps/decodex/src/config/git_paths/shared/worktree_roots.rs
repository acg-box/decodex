use std::path::{Path, PathBuf};

use crate::{
	config::{git_paths, git_paths::worktree_list, path_resolution},
	prelude::Result,
};

pub(in crate::config::git_paths::shared) fn repo_root_from_git_worktree_list(
	cwd: &Path,
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	for path in worktree_list::git_worktree_roots(cwd)? {
		let path = path_resolution::canonicalize_path_best_effort(&path);

		if path == worktree_root || path == common_dir {
			continue;
		}
		if git_paths::git_absolute_rev_parse(&path, "git-common-dir")?
			.map(|path| path_resolution::canonicalize_path_best_effort(&path))
			.as_deref()
			!= Some(common_dir)
		{
			continue;
		}
		if git_paths::git_absolute_rev_parse(&path, "git-dir")?
			.map(|path| path_resolution::canonicalize_path_best_effort(&path))
			.as_deref()
			== Some(common_dir)
		{
			return Ok(Some(path));
		}
	}

	Ok(None)
}
