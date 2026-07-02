use std::{path::Path, process::Command};

use crate::{
	git_credentials::GitCredentialEnvironment,
	prelude::{Result, eyre},
};

pub(in crate::default_branch_sync) fn run_git_capture(
	cwd: &Path,
	args: &[&str],
	git_env: &GitCredentialEnvironment,
) -> Result<String> {
	let output = build_git_command(cwd, args, git_env).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		eyre::bail!("`git {}` failed in `{}`: {detail}", args.join(" "), cwd.display());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(in crate::default_branch_sync) fn run_git_checked(
	cwd: &Path,
	args: &[&str],
	action: String,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let output = build_git_command(cwd, args, git_env).output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	if detail.is_empty() {
		eyre::bail!("Failed to {action} in `{}`.", cwd.display());
	}

	eyre::bail!("Failed to {action} in `{}`: {detail}", cwd.display());
}

pub(in crate::default_branch_sync) fn build_git_command(
	cwd: &Path,
	args: &[&str],
	git_env: &GitCredentialEnvironment,
) -> Command {
	let mut command = Command::new("git");

	git_env.apply_to(&mut command);
	command.arg("-C").arg(cwd).args(args);

	command
}
