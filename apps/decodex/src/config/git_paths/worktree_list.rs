use std::{
	path::{Path, PathBuf},
	process::Command,
};

use crate::{config::git_paths::bytes, prelude::Result};

pub(crate) fn git_worktree_roots(cwd: &Path) -> Result<Vec<PathBuf>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["worktree", "list", "--porcelain", "-z"])
		.output()?;

	if !output.status.success() {
		return Ok(Vec::new());
	}

	self::parse_git_worktree_list(&output.stdout)
}

fn parse_git_worktree_list(output: &[u8]) -> Result<Vec<PathBuf>> {
	let mut roots = Vec::new();

	for entry in output.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
		let Some(path_bytes) = entry.strip_prefix(b"worktree ") else {
			continue;
		};
		let Some(path) = bytes::path_buf_from_git_bytes(path_bytes)? else {
			continue;
		};

		roots.push(path);
	}

	Ok(roots)
}
