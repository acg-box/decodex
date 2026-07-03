use std::{
	path::Path,
	process::Command,
	thread,
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde::Deserialize;

use crate::{
	github::{self},
	prelude::{Result, eyre},
};

#[derive(Debug, Deserialize)]
pub(crate) struct PullRequestMergeViewResponse {
	pub(crate) state: String,
	#[serde(rename = "headRefOid")]
	pub(crate) head_ref_oid: Option<String>,
	#[serde(rename = "mergeCommit")]
	merge_commit: Option<PullRequestMergeCommit>,
}

#[derive(Debug, Deserialize)]
struct PullRequestMergeCommit {
	oid: String,
}

#[derive(Debug, Deserialize)]
struct CommitViewResponse {
	commit: CommitViewCommit,
}

#[derive(Debug, Deserialize)]
struct CommitViewCommit {
	message: String,
}

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

pub(crate) fn inspect_pull_request_merge_commit(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let response = inspect_pull_request_merge_response(cwd, pr_url, github_token, gh_command_path)?;

	if response.state != "MERGED" {
		eyre::bail!("Pull request `{pr_url}` did not reach `MERGED` state after landing.");
	}

	let Some(merge_commit) = response.merge_commit else {
		eyre::bail!("Pull request `{pr_url}` does not expose a merge commit after merge.");
	};

	Ok(merge_commit.oid)
}

pub(crate) fn wait_for_pull_request_merge_commit(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	timeout: Duration,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let deadline = Instant::now() + timeout;

	loop {
		match inspect_pull_request_merge_commit(cwd, pr_url, github_token, gh_command_path) {
			Ok(merge_commit) => return Ok(merge_commit),
			Err(error) if Instant::now() >= deadline => return Err(error),
			Err(error) if merge_commit_wait_error_is_retryable(&error) => {},
			Err(error) => return Err(error),
		};

		thread::sleep(Duration::from_secs(1));
	}
}

pub(crate) fn inspect_commit_subject(
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

pub(crate) fn wait_for_commit_subject(
	cwd: &Path,
	pr_url: &str,
	commit_oid: &str,
	github_token: &str,
	timeout: Duration,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let deadline = Instant::now() + timeout;

	loop {
		match inspect_commit_subject(cwd, pr_url, commit_oid, github_token, gh_command_path) {
			Ok(subject) => return Ok(subject),
			Err(error) if Instant::now() >= deadline => return Err(error),
			Err(error) if commit_subject_wait_error_is_retryable(&error) => {},
			Err(error) => return Err(error),
		};

		thread::sleep(Duration::from_secs(1));
	}
}

pub(crate) fn pull_request_is_merged_at_head(
	cwd: &Path,
	pr_url: &str,
	expected_head_sha: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<bool> {
	let response = inspect_pull_request_merge_readback(cwd, pr_url, github_token, gh_command_path)?;

	Ok(response.state == "MERGED" && response.head_ref_oid.as_deref() == Some(expected_head_sha))
}

pub(crate) fn inspect_pull_request_merge_readback(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestMergeViewResponse> {
	inspect_pull_request_merge_response(cwd, pr_url, github_token, gh_command_path)
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

pub(crate) fn merge_commit_wait_error_is_retryable(error: &Report) -> bool {
	let message = error.to_string();

	message.contains("did not reach `MERGED` state after landing")
		|| message.contains("does not expose a merge commit after merge")
}

pub(crate) fn commit_subject_wait_error_is_retryable(error: &Report) -> bool {
	let message = error.to_string().to_ascii_lowercase();

	message.contains("failed to inspect merge commit")
		&& (message.contains("not found") || message.contains("http 404"))
}

fn inspect_pull_request_merge_response(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestMergeViewResponse> {
	let mut command = github::gh_command_with_config(gh_command_path);

	command.args(["pr", "view", pr_url, "--json", "state,headRefOid,mergeCommit"]);
	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to inspect merge result for `{pr_url}`: {}", stderr.trim());
	}

	serde_json::from_slice::<PullRequestMergeViewResponse>(&output.stdout).map_err(Into::into)
}
