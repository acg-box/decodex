use std::path::Path;

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
impl PullRequestMergeViewResponse {
	pub(in crate::github::merge_readback) fn merge_commit_oid(&self) -> Option<&str> {
		self.merge_commit.as_ref().map(|commit| commit.oid.as_str())
	}
}

#[derive(Debug, Deserialize)]
pub(in crate::github::merge_readback) struct CommitViewResponse {
	pub(in crate::github::merge_readback) commit: CommitViewCommit,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::merge_readback) struct CommitViewCommit {
	pub(in crate::github::merge_readback) message: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestMergeCommit {
	oid: String,
}

pub(in crate::github::merge_readback) fn inspect_pull_request_merge_response(
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
