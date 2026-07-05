use crate::orchestrator::status::post_review::{Command, Path};

pub(crate) fn worktree_has_no_tracked_changes(worktree_path: &Path) -> bool {
	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain", "--untracked-files=no"])
		.output()
	else {
		return false;
	};

	output.status.success() && String::from_utf8_lossy(&output.stdout).trim().is_empty()
}
