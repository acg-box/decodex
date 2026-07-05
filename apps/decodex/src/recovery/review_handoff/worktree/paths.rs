use std::path::Path;

use crate::recovery::{context::RecoveryContext, git_worktree};

pub(in crate::recovery) fn relative_worktree_path_for_recovery(
	context: &RecoveryContext,
	worktree_path: &Path,
) -> Option<String> {
	git_worktree::repository_relative_path(context.config.repo_root(), worktree_path).or_else(
		|| {
			worktree_path
				.strip_prefix(context.config.repo_root())
				.ok()
				.map(|relative| relative.to_string_lossy().to_string())
		},
	)
}
