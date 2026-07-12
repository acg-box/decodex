use std::{path::Path, process::Command};

use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	github::{self},
	prelude::{Result, eyre},
};

#[derive(Debug, Deserialize)]
struct IssueCommentCreateResponse {
	id: i64,
	#[serde(rename = "created_at")]
	created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
struct IssueCommentReadResponse {
	id: i64,
	body: String,
	#[serde(rename = "created_at")]
	created_at: String,
}

#[allow(dead_code)]
pub(crate) fn ensure_pull_request_issue_comment(
	cwd: &Path,
	pr_url: &str,
	marker: &str,
	body: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<(i64, i64, bool)> {
	if marker.trim().is_empty() || !body.contains(marker) {
		eyre::bail!("GitHub idempotency marker must be non-empty and present in the comment body.");
	}
	let locator = github::parse_pull_request_url(pr_url)?;
	let endpoint = format!(
		"repos/{}/{}/issues/{}/comments?per_page=100",
		locator.owner, locator.repo, locator.number
	);
	let mut command = github::gh_command_with_config(gh_command_path);
	configure_issue_comments_list_command(&mut command, &endpoint);
	command.current_dir(cwd);
	github::configure_gh_command(&mut command, github_token);
	let output = command.output()?;
	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		eyre::bail!("Failed to inspect pull request comments on `{pr_url}`: {}", stderr.trim());
	}
	if let Some((id, created_at)) =
		find_issue_comment_marker_in_slurped_pages(&output.stdout, marker, pr_url)?
	{
		return Ok((id, created_at, false));
	}
	let (id, created_at) =
		post_pull_request_issue_comment(cwd, pr_url, body, github_token, gh_command_path)?;
	Ok((id, created_at, true))
}

pub(in crate::github) fn find_issue_comment_marker_in_slurped_pages(
	payload: &[u8],
	marker: &str,
	pr_url: &str,
) -> Result<Option<(i64, i64)>> {
	let pages = serde_json::from_slice::<Vec<Vec<IssueCommentReadResponse>>>(payload)?;
	pages
		.into_iter()
		.flatten()
		.find(|comment| comment.body.contains(marker))
		.map(|comment| Ok((comment.id, parse_comment_timestamp(&comment.created_at, pr_url)?)))
		.transpose()
}

pub(in crate::github) fn configure_issue_comments_list_command(
	command: &mut Command,
	endpoint: &str,
) {
	command.args(["api", "--paginate", "--slurp", endpoint]);
}

pub(crate) fn post_pull_request_issue_comment(
	cwd: &Path,
	pr_url: &str,
	body: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<(i64, i64)> {
	let locator = github::parse_pull_request_url(pr_url)?;
	let endpoint =
		format!("repos/{}/{}/issues/{}/comments", locator.owner, locator.repo, locator.number);
	let mut command = github::gh_command_with_config(gh_command_path);

	command.args(["api", endpoint.as_str(), "-f", &format!("body={body}")]);
	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to post pull request comment on `{pr_url}`: {}", stderr.trim());
	}

	let response = serde_json::from_slice::<IssueCommentCreateResponse>(&output.stdout)?;
	let created_at_unix_epoch = parse_comment_timestamp(&response.created_at, pr_url)?;

	Ok((response.id, created_at_unix_epoch))
}

fn parse_comment_timestamp(created_at: &str, pr_url: &str) -> Result<i64> {
	Ok(OffsetDateTime::parse(created_at, &Rfc3339)
		.map_err(|error| {
			eyre::eyre!(
				"Failed to parse GitHub comment timestamp `{}` for `{pr_url}`: {error}",
				created_at
			)
		})?
		.unix_timestamp())
}
