use std::{path::Path, process::Command};

use crate::{
	github::{self},
	prelude::{Result, eyre},
};

pub(crate) fn admin_merge_pull_request(
	cwd: &Path,
	pr_url: &str,
	reviewed_head_sha: &str,
	merge_subject: Option<&str>,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	let mut command = github::gh_command_with_config(gh_command_path);

	configure_admin_merge_command(&mut command, pr_url, reviewed_head_sha, merge_subject);

	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	if detail.is_empty() {
		eyre::bail!("Failed to admin-merge `{pr_url}`.");
	}

	eyre::bail!("Failed to admin-merge `{pr_url}`: {detail}");
}

pub(crate) fn configure_admin_merge_command(
	command: &mut Command,
	pr_url: &str,
	reviewed_head_sha: &str,
	merge_subject: Option<&str>,
) {
	command.args(["pr", "merge", "--admin", "--merge", "--match-head-commit", reviewed_head_sha]);

	if let Some(merge_subject) = merge_subject {
		command.args(["--subject", merge_subject]);
	}

	command.args(["--body", ""]);
	command.arg(pr_url);
}
