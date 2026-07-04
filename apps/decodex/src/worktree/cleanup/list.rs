use std::{
	fs,
	path::{Path, PathBuf},
};

use crate::{prelude::Result, worktree::git};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinkedWorktree {
	pub(crate) branch_name: String,
	pub(crate) path: PathBuf,
}

pub(crate) fn linked_worktrees(repo_root: &Path) -> Result<Vec<LinkedWorktree>> {
	Ok(parse_linked_worktrees(&git::git_stdout(
		repo_root,
		["worktree", "list", "--porcelain"],
		"list linked worktrees",
	)?))
}

pub(crate) fn linked_worktree_under_root(path: &Path, worktree_root: &Path) -> Result<Option<()>> {
	if !path.exists() || !worktree_root.exists() {
		return Ok(None);
	}

	let canonical_path = fs::canonicalize(path)?;
	let canonical_root = fs::canonicalize(worktree_root)?;

	if canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root {
		return Ok(Some(()));
	}

	Ok(None)
}

fn parse_linked_worktrees(output: &str) -> Vec<LinkedWorktree> {
	let mut entries = Vec::new();
	let mut current_path: Option<PathBuf> = None;
	let mut current_branch: Option<String> = None;

	for line in output.lines() {
		if line.is_empty() {
			push_linked_worktree_entry(&mut entries, &mut current_path, &mut current_branch);

			continue;
		}

		if let Some(path) = line.strip_prefix("worktree ") {
			push_linked_worktree_entry(&mut entries, &mut current_path, &mut current_branch);

			current_path = Some(PathBuf::from(path));

			continue;
		}
		if let Some(branch_ref) = line.strip_prefix("branch refs/heads/") {
			current_branch = Some(branch_ref.to_owned());
		}
	}

	push_linked_worktree_entry(&mut entries, &mut current_path, &mut current_branch);

	entries
}

fn push_linked_worktree_entry(
	entries: &mut Vec<LinkedWorktree>,
	path: &mut Option<PathBuf>,
	branch_name: &mut Option<String>,
) {
	if let (Some(path), Some(branch_name)) = (path.take(), branch_name.take()) {
		entries.push(LinkedWorktree { branch_name, path });
	}

	*path = None;
	*branch_name = None;
}
