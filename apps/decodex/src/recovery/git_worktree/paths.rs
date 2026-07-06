use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::prelude::{Result, eyre};

pub(in crate::recovery) fn git_toplevel_path(cwd: &Path) -> Result<PathBuf> {
	let output =
		Command::new("git").arg("-C").arg(cwd).args(["rev-parse", "--show-toplevel"]).output()?;

	if output.status.success() {
		return Ok(PathBuf::from(super::trimmed_stdout(&output.stdout)?));
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect current Git worktree root from `{}`: {}",
		cwd.display(),
		stderr.trim()
	)
}

pub(in crate::recovery) fn worktree_checkout_branch_name(
	worktree_path: &Path,
) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "HEAD"])
		.output()?;

	if output.status.success() {
		return Ok(Some(super::trimmed_stdout(&output.stdout)?));
	}
	if output.status.code() == Some(1) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect retained worktree branch in `{}`: {}",
		worktree_path.display(),
		stderr.trim()
	)
}

pub(in crate::recovery) fn repository_relative_path(
	repo_root: &Path,
	path: &Path,
) -> Option<String> {
	let canonical_repo_root = fs::canonicalize(repo_root).ok()?;
	let canonical_path = fs::canonicalize(path).ok()?;
	let relative = canonical_path.strip_prefix(canonical_repo_root).ok()?;

	Some(relative.to_string_lossy().to_string())
}
