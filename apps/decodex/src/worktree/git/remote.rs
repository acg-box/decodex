use std::{fs, path::Path, process::Command};

use crate::{
	prelude::{Result, eyre},
	worktree::git::command,
};

pub(in crate::worktree) fn normalize_origin_remote_for_worktrees(repo_root: &Path) -> Result<()> {
	let Some(origin_url) = command::try_git_stdout(
		repo_root,
		["remote", "get-url", "origin"],
		"read source repository origin remote",
	)?
	else {
		return Ok(());
	};

	if !is_relative_filesystem_remote(origin_url.as_str()) {
		return Ok(());
	}

	let absolute_origin = fs::canonicalize(repo_root.join(&origin_url))?;
	let absolute_origin = absolute_origin.to_str().ok_or_else(|| {
		eyre::eyre!(
			"Resolved absolute origin path `{}` is not valid UTF-8.",
			absolute_origin.display()
		)
	})?;

	command::run_git(
		repo_root,
		["remote", "set-url", "origin", absolute_origin],
		"normalize the source repository origin remote for linked worktrees",
	)
}

pub(in crate::worktree) fn is_relative_filesystem_remote(remote_url: &str) -> bool {
	if remote_url.starts_with("./") || remote_url.starts_with("../") {
		return true;
	}
	if remote_url == "~" || remote_url.starts_with("~/") {
		return false;
	}

	!remote_url.contains("://") && !remote_url.contains(':') && !Path::new(remote_url).is_absolute()
}

pub(in crate::worktree) fn fetch_remote_branch_if_present(
	repo_root: &Path,
	branch_name: &str,
) -> Result<bool> {
	if command::try_git_stdout(
		repo_root,
		["remote", "get-url", "origin"],
		"read source repository origin remote",
	)?
	.is_none()
	{
		return Ok(false);
	}

	let remote_ref = format!("refs/heads/{branch_name}");
	let mut branch_check = Command::new("git");

	command::configure_noninteractive_git(&mut branch_check);

	let branch_check = branch_check
		.arg("-C")
		.arg(repo_root)
		.args(["ls-remote", "--exit-code", "--heads", "origin", remote_ref.as_str()])
		.output()?;

	if !branch_check.status.success() {
		if branch_check.status.code() == Some(2) {
			return Ok(false);
		}

		let stderr = String::from_utf8_lossy(&branch_check.stderr);

		eyre::bail!(
			"Failed to inspect remote worktree branch `{branch_name}` in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let remote_tracking_ref = format!("refs/remotes/origin/{branch_name}");
	let mut fetch = Command::new("git");

	command::configure_noninteractive_git(&mut fetch);

	let output = fetch
		.arg("-C")
		.arg(repo_root)
		.args([
			"fetch",
			"--quiet",
			"--no-tags",
			"origin",
			&format!("refs/heads/{branch_name}:{remote_tracking_ref}"),
		])
		.output()?;

	if output.status.success() {
		return Ok(true);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to fetch remote worktree branch `{branch_name}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}
