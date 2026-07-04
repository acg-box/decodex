use std::path::Path;

use crate::{
	github::{
		merge_readback::response::CommitViewResponse,
		{self},
	},
	prelude::{Result, eyre},
};

pub(in crate::github::merge_readback) fn inspect_commit_subject(
	cwd: &Path,
	pr_url: &str,
	commit_oid: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let locator = github::parse_pull_request_url(pr_url)?;
	let mut command = github::gh_command_with_config(gh_command_path);

	command
		.args(["api", &format!("repos/{}/{}/commits/{}", locator.owner, locator.repo, commit_oid)]);
	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect merge commit `{commit_oid}` for `{pr_url}`: {}",
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<CommitViewResponse>(&output.stdout)?;
	let subject = response
		.commit
		.message
		.lines()
		.next()
		.map(|line| line.trim_end_matches('\r'))
		.unwrap_or_default();

	if subject.is_empty() {
		eyre::bail!("Merge commit `{commit_oid}` for `{pr_url}` does not expose a subject line.");
	}

	Ok(subject.to_owned())
}
