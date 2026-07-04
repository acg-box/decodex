mod branches;
mod list;
mod model;

pub(crate) use self::{
	branches::infer_default_branch_name,
	model::{MergedWorktreeCleanliness, MergedWorktreeCleanupDebt},
};

use std::path::Path;

use crate::prelude::Result;

pub(crate) fn merged_worktree_cleanup_debts(
	repo_root: &Path,
	worktree_root: &Path,
	default_branch: &str,
) -> Result<Vec<MergedWorktreeCleanupDebt>> {
	if default_branch.is_empty() || !worktree_root.exists() {
		return Ok(Vec::new());
	}

	let mut debts = Vec::new();

	for worktree in list::linked_worktrees(repo_root)? {
		if worktree.branch_name == default_branch
			|| list::linked_worktree_under_root(&worktree.path, worktree_root)?.is_none()
			|| branches::branch_merged_into_default(
				repo_root,
				&worktree.branch_name,
				default_branch,
			)?
			.is_none()
		{
			continue;
		}

		debts.push(MergedWorktreeCleanupDebt {
			branch_name: worktree.branch_name,
			cleanliness: branches::worktree_cleanliness(&worktree.path)?,
			default_branch: default_branch.to_owned(),
			path: worktree.path,
		});
	}

	debts.sort_by(|left, right| {
		left.path.cmp(&right.path).then_with(|| left.branch_name.cmp(&right.branch_name))
	});

	Ok(debts)
}
