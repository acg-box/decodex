use std::path::Path;

use serde::Deserialize;

use crate::prelude::{Result, eyre};

use super::{configure_gh_command, gh_command_with_config, parse_pull_request_url};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepositoryContext {
	pub(crate) owner: String,
	pub(crate) name: String,
	pub(crate) default_branch: String,
	pub(crate) merge_commit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct RepositoryViewResponse {
	name: String,
	owner: RepositoryViewOwner,
	#[serde(rename = "defaultBranchRef")]
	default_branch_ref: RepositoryViewBranchRef,
	#[serde(rename = "mergeCommitAllowed")]
	merge_commit_allowed: bool,
}

#[derive(Debug, Deserialize)]
struct RepositoryViewOwner {
	login: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryViewBranchRef {
	name: String,
}

pub(crate) fn inspect_repository_context(
	cwd: &Path,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<RepositoryContext> {
	let mut command = gh_command_with_config(gh_command_path);

	command.args(["repo", "view", "--json", "name,owner,defaultBranchRef,mergeCommitAllowed"]);
	command.current_dir(cwd);

	configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to inspect current GitHub repository context: {}", stderr.trim());
	}

	let response = serde_json::from_slice::<RepositoryViewResponse>(&output.stdout)?;

	Ok(RepositoryContext {
		owner: response.owner.login,
		name: response.name,
		default_branch: response.default_branch_ref.name,
		merge_commit_allowed: response.merge_commit_allowed,
	})
}

pub(crate) fn pull_request_matches_repository(
	pr_url: &str,
	repository: &RepositoryContext,
) -> Result<bool> {
	let locator = parse_pull_request_url(pr_url)?;

	Ok(locator.owner.eq_ignore_ascii_case(&repository.owner)
		&& locator.repo.eq_ignore_ascii_case(&repository.name))
}
