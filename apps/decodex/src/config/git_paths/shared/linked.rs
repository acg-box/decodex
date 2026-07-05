use std::path::{Path, PathBuf};

use crate::{
	config::git_paths::shared::{search, worktree_roots},
	prelude::Result,
};

pub(in crate::config::git_paths::shared) fn shared_repo_root_for_linked_worktree(
	cwd: &Path,
	worktree_root: Option<&Path>,
	common_dir: Option<&Path>,
) -> Result<Option<PathBuf>> {
	let Some(worktree_root) = worktree_root else {
		return Ok(None);
	};
	let Some(common_dir) = common_dir else {
		return Ok(None);
	};

	if let Some(shared_repo_root) =
		worktree_roots::repo_root_from_git_worktree_list(cwd, common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}
	if let Some(shared_repo_root) =
		search::repo_root_from_gitdir_reference_search(common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}

	Ok(None)
}
