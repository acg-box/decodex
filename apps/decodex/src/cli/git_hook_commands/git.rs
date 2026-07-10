use std::process::Command;

use crate::prelude::{Result, eyre};

pub(in crate::cli::git_hook_commands) fn run_git_lines_if_success(
	args: &[String],
) -> Result<Option<Vec<String>>> {
	let output = Command::new("git").args(args).output()?;

	if !output.status.success() {
		return Ok(None);
	}

	let stdout = String::from_utf8(output.stdout)?;

	Ok(Some(stdout.lines().map(ToOwned::to_owned).collect()))
}

pub(in crate::cli::git_hook_commands) fn run_git_lines(args: &[String]) -> Result<Vec<String>> {
	let output = Command::new("git").args(args).output()?;

	if !output.status.success() {
		eyre::bail!(
			"`git {}` failed: {}",
			args.join(" "),
			String::from_utf8_lossy(&output.stderr).trim()
		);
	}

	let stdout = String::from_utf8(output.stdout)?;

	Ok(stdout.lines().map(ToOwned::to_owned).collect())
}
