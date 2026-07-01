use std::{path::Path, process::Command};

use crate::prelude::{Result, eyre};

pub(crate) fn delete_local_branch_if_present(repo_root: &Path, branch_name: &str) -> Result<()> {
	let local_ref = format!("refs/heads/{branch_name}");
	let branch_check = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", local_ref.as_str()])
		.output()?;

	if !branch_check.status.success() {
		if branch_check.status.code() == Some(1) {
			return Ok(());
		}

		let stderr = String::from_utf8_lossy(&branch_check.stderr);

		eyre::bail!(
			"Failed to inspect retained local branch `{branch_name}` in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let delete_output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["branch", "-D", branch_name])
		.output()?;

	if delete_output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&delete_output.stderr);

	if stderr.contains("not found") || stderr.contains("branch not found") {
		return Ok(());
	}

	eyre::bail!(
		"Failed to delete retained local branch `{branch_name}` from `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

pub(crate) fn detach_worktree_head_from_branch_if_checked_out(
	worktree_path: &Path,
	branch_name: &str,
) -> Result<()> {
	let head_ref = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "HEAD"])
		.output()?;

	if !head_ref.status.success() {
		if head_ref.status.code() == Some(1) {
			return Ok(());
		}

		let stderr = String::from_utf8_lossy(&head_ref.stderr);

		eyre::bail!(
			"Failed to inspect retained worktree HEAD in `{}` before local branch cleanup: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let current_branch = String::from_utf8(head_ref.stdout)
		.map_err(|error| {
			eyre::eyre!(
				"Retained worktree HEAD in `{}` is not valid UTF-8: {error}",
				worktree_path.display()
			)
		})?
		.trim()
		.to_owned();

	if current_branch != branch_name {
		return Ok(());
	}

	let detach_output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["checkout", "--quiet", "--detach"])
		.output()?;

	if detach_output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&detach_output.stderr);

	eyre::bail!(
		"Failed to detach retained worktree `{}` from branch `{branch_name}` before local branch cleanup: {}",
		worktree_path.display(),
		stderr.trim()
	);
}
