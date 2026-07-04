pub(crate) mod bytes;
pub(crate) mod worktree_list;

mod shared;

pub(super) use bytes::path_buf_from_git_line_output;

use std::{
	path::{Path, PathBuf},
	process::Command,
};

use crate::{config::path_resolution, prelude::Result};

/// Canonical repository root for the current Git checkout.
pub fn canonical_repo_root_for_checkout(cwd: &Path) -> Result<Option<PathBuf>> {
	let worktree_root = git_absolute_rev_parse(cwd, "show-toplevel")?
		.map(|path| path_resolution::canonicalize_path_best_effort(&path));

	if let Some(shared_repo_root) =
		shared::shared_repo_root_for_checkout(cwd, worktree_root.as_deref())?
	{
		return Ok(Some(shared_repo_root));
	}

	Ok(worktree_root)
}

/// Absolute Git administrative directory for the current checkout.
pub fn git_dir_for_checkout(cwd: &Path) -> Result<Option<PathBuf>> {
	Ok(git_absolute_rev_parse(cwd, "git-dir")?
		.map(|path| path_resolution::canonicalize_path_best_effort(&path)))
}

/// Whether two Git checkouts belong to the same shared repository.
pub fn checkouts_share_repository(a: &Path, b: &Path) -> Result<bool> {
	let a_common_dir = git_absolute_rev_parse(a, "git-common-dir")?
		.map(|path| path_resolution::canonicalize_path_best_effort(&path));
	let b_common_dir = git_absolute_rev_parse(b, "git-common-dir")?
		.map(|path| path_resolution::canonicalize_path_best_effort(&path));

	Ok(a_common_dir.is_some() && a_common_dir == b_common_dir)
}

pub(crate) fn git_absolute_rev_parse(cwd: &Path, mode: &str) -> Result<Option<PathBuf>> {
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
