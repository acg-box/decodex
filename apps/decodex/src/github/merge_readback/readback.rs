use std::path::Path;

use crate::{
	github::merge_readback::response::{self, PullRequestMergeViewResponse},
	prelude::{Result, eyre},
};

pub(crate) fn inspect_pull_request_merge_commit(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let response =
		response::inspect_pull_request_merge_response(cwd, pr_url, github_token, gh_command_path)?;

	if response.state != "MERGED" {
		eyre::bail!("Pull request `{pr_url}` did not reach `MERGED` state after landing.");
	}

	let Some(merge_commit_oid) = response.merge_commit_oid() else {
		eyre::bail!("Pull request `{pr_url}` does not expose a merge commit after merge.");
	};

	Ok(merge_commit_oid.to_owned())
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
	response::inspect_pull_request_merge_response(cwd, pr_url, github_token, gh_command_path)
}
