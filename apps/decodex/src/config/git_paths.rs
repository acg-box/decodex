#[cfg(unix)] use std::os::unix::ffi::OsStringExt as _;
use std::{
	ffi::OsString,
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process::Command,
};

use super::canonicalize_path_best_effort;
use crate::prelude::{Result, eyre};

/// Canonical repository root for the current Git checkout.
pub fn canonical_repo_root_for_checkout(cwd: &Path) -> Result<Option<PathBuf>> {
	let worktree_root = git_absolute_rev_parse(cwd, "show-toplevel")?
		.map(|path| canonicalize_path_best_effort(&path));

	if let Some(shared_repo_root) = shared_repo_root_for_checkout(cwd, worktree_root.as_deref())? {
		return Ok(Some(shared_repo_root));
	}

	Ok(worktree_root)
}

/// Absolute Git administrative directory for the current checkout.
pub fn git_dir_for_checkout(cwd: &Path) -> Result<Option<PathBuf>> {
	Ok(git_absolute_rev_parse(cwd, "git-dir")?.map(|path| canonicalize_path_best_effort(&path)))
}

/// Whether two Git checkouts belong to the same shared repository.
pub fn checkouts_share_repository(a: &Path, b: &Path) -> Result<bool> {
	let a_common_dir = git_absolute_rev_parse(a, "git-common-dir")?
		.map(|path| canonicalize_path_best_effort(&path));
	let b_common_dir = git_absolute_rev_parse(b, "git-common-dir")?
		.map(|path| canonicalize_path_best_effort(&path));

	Ok(a_common_dir.is_some() && a_common_dir == b_common_dir)
}

fn shared_repo_root_for_checkout(
	cwd: &Path,
	worktree_root: Option<&Path>,
) -> Result<Option<PathBuf>> {
	let git_dir =
		git_absolute_rev_parse(cwd, "git-dir")?.map(|path| canonicalize_path_best_effort(&path));
	let common_dir = git_absolute_rev_parse(cwd, "git-common-dir")?
		.map(|path| canonicalize_path_best_effort(&path));
	let prefers_shared_repo_root = git_dir.is_some() && git_dir != common_dir;

	if prefers_shared_repo_root {
		return shared_repo_root_for_linked_worktree(cwd, worktree_root, common_dir.as_deref());
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
		repo_root_from_git_worktree_list(cwd, common_dir, worktree_root)?
	{
		return Ok(Some(shared_repo_root));
	}
	if let Some(shared_repo_root) =
		repo_root_from_gitdir_reference_search(common_dir, worktree_root)?
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
	for path in git_worktree_roots(cwd)? {
		let path = canonicalize_path_best_effort(&path);

		if path == worktree_root || path == common_dir {
			continue;
		}
		if git_absolute_rev_parse(&path, "git-common-dir")?
			.map(|path| canonicalize_path_best_effort(&path))
			.as_deref()
			!= Some(common_dir)
		{
			continue;
		}
		if git_absolute_rev_parse(&path, "git-dir")?
			.map(|path| canonicalize_path_best_effort(&path))
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
	let Some(search_root) = nearest_shared_ancestor(common_dir, worktree_root) else {
		return Ok(None);
	};

	find_checkout_root_referencing_common_dir(&search_root, common_dir, worktree_root)
}

fn git_worktree_roots(cwd: &Path) -> Result<Vec<PathBuf>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["worktree", "list", "--porcelain", "-z"])
		.output()?;

	if !output.status.success() {
		return Ok(Vec::new());
	}

	parse_git_worktree_list(&output.stdout)
}

fn parse_git_worktree_list(output: &[u8]) -> Result<Vec<PathBuf>> {
	let mut roots = Vec::new();

	for entry in output.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
		let Some(path_bytes) = entry.strip_prefix(b"worktree ") else {
			continue;
		};
		let Some(path) = path_buf_from_git_bytes(path_bytes)? else {
			continue;
		};

		roots.push(path);
	}

	Ok(roots)
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
			&& git_dir_reference_matches_common_dir_best_effort(&path.join(".git"), common_dir)
		{
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
	git_dir_reference_matches_common_dir(dot_git, common_dir).unwrap_or_default()
}

fn git_dir_reference_matches_common_dir(dot_git: &Path, common_dir: &Path) -> Result<bool> {
	if dot_git.is_dir() {
		return Ok(fs::canonicalize(dot_git)? == common_dir);
	}
	if !dot_git.is_file() {
		return Ok(false);
	}

	let gitdir = parse_gitdir_file(dot_git)?;

	Ok(fs::canonicalize(gitdir)? == common_dir)
}

fn parse_gitdir_file(dot_git: &Path) -> Result<PathBuf> {
	let contents = fs::read_to_string(dot_git)?;
	let prefix = "gitdir:";
	let Some(gitdir) = contents.lines().find_map(|line| line.strip_prefix(prefix)) else {
		eyre::bail!("Git dir file `{}` is missing a `gitdir:` entry.", dot_git.display());
	};
	let gitdir = gitdir.trim();

	if gitdir.is_empty() {
		eyre::bail!("Git dir file `{}` has an empty `gitdir:` entry.", dot_git.display());
	}

	let gitdir = PathBuf::from(gitdir);

	if gitdir.is_absolute() {
		return Ok(gitdir);
	}

	let Some(parent) = dot_git.parent() else {
		eyre::bail!("Git dir file `{}` must have a parent directory.", dot_git.display());
	};

	Ok(parent.join(gitdir))
}

fn git_absolute_rev_parse(cwd: &Path, mode: &str) -> Result<Option<PathBuf>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["rev-parse", "--path-format=absolute", &format!("--{mode}")])
		.output()?;

	if !output.status.success() {
		return Ok(None);
	}

	path_buf_from_git_line_output(&output.stdout)
}

pub(super) fn path_buf_from_git_line_output(output: &[u8]) -> Result<Option<PathBuf>> {
	let resolved = output.strip_suffix(b"\n").unwrap_or(output);
	let resolved = resolved.strip_suffix(b"\r").unwrap_or(resolved);

	path_buf_from_git_bytes(resolved)
}

#[cfg(unix)]
fn path_buf_from_git_bytes(path: &[u8]) -> Result<Option<PathBuf>> {
	if path.is_empty() {
		return Ok(None);
	}

	Ok(Some(PathBuf::from(OsString::from_vec(path.to_vec()))))
}

#[cfg(not(unix))]
fn path_buf_from_git_bytes(path: &[u8]) -> Result<Option<PathBuf>> {
	let resolved = String::from_utf8(path.to_vec())?;

	if resolved.is_empty() {
		return Ok(None);
	}

	Ok(Some(PathBuf::from(resolved)))
}
