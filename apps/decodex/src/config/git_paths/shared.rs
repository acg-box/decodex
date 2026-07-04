use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use crate::{
	config::{self, git_paths, git_paths::worktree_list},
	prelude::Result,
};

pub(crate) fn shared_repo_root_for_checkout(
	cwd: &Path,
	worktree_root: Option<&Path>,
) -> Result<Option<PathBuf>> {
	let git_dir = git_paths::git_absolute_rev_parse(cwd, "git-dir")?
		.map(|path| config::canonicalize_path_best_effort(&path));
	let common_dir = git_paths::git_absolute_rev_parse(cwd, "git-common-dir")?
		.map(|path| config::canonicalize_path_best_effort(&path));
	let prefers_shared_repo_root = git_dir.is_some() && git_dir != common_dir;

	if prefers_shared_repo_root {
		return self::shared_repo_root_for_linked_worktree(
			cwd,
			worktree_root,
			common_dir.as_deref(),
		);
	}

	Ok(None)
}

fn shared_repo_root_for_linked_worktree(
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
		self::repo_root_from_git_worktree_list(cwd, common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}
	if let Some(shared_repo_root) =
		self::repo_root_from_gitdir_reference_search(common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}

	Ok(None)
}

fn repo_root_from_git_worktree_list(
	cwd: &Path,
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	for path in worktree_list::git_worktree_roots(cwd)? {
		let path = config::canonicalize_path_best_effort(&path);

		if path == worktree_root || path == common_dir {
			continue;
		}
		if git_paths::git_absolute_rev_parse(&path, "git-common-dir")?
			.map(|path| config::canonicalize_path_best_effort(&path))
			.as_deref()
			!= Some(common_dir)
		{
			continue;
		}
		if git_paths::git_absolute_rev_parse(&path, "git-dir")?
			.map(|path| config::canonicalize_path_best_effort(&path))
			.as_deref()
			== Some(common_dir)
		{
			return Ok(Some(path));
		}
	}

	Ok(None)
}

fn repo_root_from_gitdir_reference_search(
	common_dir: &Path,
	worktree_root: &Path,
) -> Result<Option<PathBuf>> {
	let Some(search_root) = self::nearest_shared_ancestor(common_dir, worktree_root) else {
		return Ok(None);
	};

	self::find_checkout_root_referencing_common_dir(&search_root, common_dir, worktree_root)
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
			&& self::git_dir_reference_matches_common_dir_best_effort(
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

fn git_dir_reference_matches_common_dir_best_effort(dot_git: &Path, common_dir: &Path) -> bool {
	self::git_dir_reference_matches_common_dir(dot_git, common_dir).unwrap_or_default()
}

fn git_dir_reference_matches_common_dir(dot_git: &Path, common_dir: &Path) -> Result<bool> {
	if dot_git.is_dir() {
		return Ok(fs::canonicalize(dot_git)? == common_dir);
	}
	if !dot_git.is_file() {
		return Ok(false);
	}

	let gitdir = self::parse_gitdir_file(dot_git)?;

	Ok(fs::canonicalize(gitdir)? == common_dir)
}

fn parse_gitdir_file(dot_git: &Path) -> Result<PathBuf> {
	let contents = fs::read_to_string(dot_git)?;
	let prefix = "gitdir:";
	let Some(gitdir) = contents.lines().find_map(|line| line.strip_prefix(prefix)) else {
		crate::prelude::eyre::bail!(
			"Git dir file `{}` is missing a `gitdir:` entry.",
			dot_git.display()
		);
	};
	let gitdir = gitdir.trim();

	if gitdir.is_empty() {
		crate::prelude::eyre::bail!(
			"Git dir file `{}` has an empty `gitdir:` entry.",
			dot_git.display()
		);
	}

	let gitdir = PathBuf::from(gitdir);

	if gitdir.is_absolute() {
		return Ok(gitdir);
	}

	let Some(parent) = dot_git.parent() else {
		crate::prelude::eyre::bail!(
			"Git dir file `{}` must have a parent directory.",
			dot_git.display()
		);
	};

	Ok(parent.join(gitdir))
}
