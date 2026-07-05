use std::{ffi::OsStr, path::Path, process::Command};

use crate::prelude::{Result, eyre};

pub(in crate::worktree) fn git_stdout<I, S>(
	repo_root: &Path,
	args: I,
	action: &str,
) -> Result<String>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(in crate::worktree) fn run_git<I, S>(repo_root: &Path, args: I, action: &str) -> Result<()>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
	}

	Ok(())
}

pub(in crate::worktree::git) fn try_git_stdout<I, S>(
	repo_root: &Path,
	args: I,
	action: &str,
) -> Result<Option<String>>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if output.status.success() {
		return Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()));
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	if stderr.contains("No such remote") {
		return Ok(None);
	}

	eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
}

pub(in crate::worktree::git) fn configure_noninteractive_git(
	command: &mut Command,
) -> &mut Command {
	command.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never")
}
