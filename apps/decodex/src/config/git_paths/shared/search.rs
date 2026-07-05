use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use crate::{config::git_paths::shared::gitdir, prelude::Result};

pub(in crate::config::git_paths::shared) fn repo_root_from_gitdir_reference_search(
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	let Some(search_root) = nearest_shared_ancestor(common_dir, worktree_root) else {
		return Ok(None);
	};

	find_checkout_root_referencing_common_dir(&search_root, common_dir, worktree_root)
}

fn nearest_shared_ancestor(a: &Path, b: &Path) -> Option<PathBuf> {
	a.ancestors().find(|ancestor| b.starts_with(ancestor)).map(Path::to_path_buf)
}

fn find_checkout_root_referencing_common_dir(
	search_root: &Path,
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	const MAX_DIRS_TO_SCAN: usize = 4_096;

	let mut stack = vec![search_root.to_path_buf()];
	let mut scanned_dirs = 0_usize;

	while let Some(path) = stack.pop() {
		if scanned_dirs >= MAX_DIRS_TO_SCAN {
			return Ok(None);
		}

		scanned_dirs += 1;

		if path != worktree_root
			&& path != common_dir
			&& gitdir::git_dir_reference_matches_common_dir_best_effort(
				&path.join(".git"),
				common_dir,
			) {
			return Ok(Some(path));
		}

		let entries = match fs::read_dir(&path) {
			Ok(entries) => entries,
			Err(error) if error.kind() == ErrorKind::NotFound => continue,
			Err(error) => return Err(error.into()),
		};

		for entry in entries {
			let entry = entry?;
			let child = entry.path();

			if !child.is_dir()
				|| child == common_dir
				|| child.starts_with(common_dir)
				|| child == worktree_root
				|| child.starts_with(worktree_root)
			{
				continue;
			}

			stack.push(child);
		}
	}

	Ok(None)
}
