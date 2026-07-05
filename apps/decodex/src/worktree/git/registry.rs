use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::{
	prelude::{Result, eyre},
	worktree::git::command,
};

pub(in crate::worktree) fn worktree_is_registered(
	repo_root: &Path,
	expected_path: &Path,
) -> Result<bool> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["worktree", "list", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to list linked worktrees in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	for line in String::from_utf8_lossy(&output.stdout).lines() {
		let Some(path) = line.strip_prefix("worktree ") else {
			continue;
		};
		let candidate = fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

		if candidate == expected_path {
			return Ok(true);
		}
	}

	Ok(false)
}

pub(in crate::worktree) fn resolve_source_repo_git_common_dir(repo_root: &Path) -> Result<PathBuf> {
	Ok(fs::canonicalize(PathBuf::from(command::git_stdout(
		repo_root,
		["rev-parse", "--path-format=absolute", "--git-common-dir"],
		"resolve source repository git common dir",
	)?))?)
}
