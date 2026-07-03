use std::path::Path;

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
	let created_at_unix_epoch = OffsetDateTime::parse(&response.created_at, &Rfc3339)
		.map_err(|error| {
			eyre::eyre!(
				"Failed to parse GitHub comment timestamp `{}` for `{pr_url}`: {error}",
				response.created_at
			)
		})?
		.unix_timestamp();

	Ok((response.id, created_at_unix_epoch))
}
