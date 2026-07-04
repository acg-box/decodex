use std::{env, path::Path};

use serde::Deserialize;

use crate::{
	agent::tracker_tool_bridge::{PullRequestDetails, PullRequestInspector, ReviewHandoffContext},
	github,
};

pub(in crate::agent::tracker_tool_bridge) struct GhPullRequestInspector;
impl PullRequestInspector for GhPullRequestInspector {
	fn inspect_pull_request(
		&self,
		cwd: &Path,
		pr_url: &str,
		github_token: &str,
		gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String> {
		let mut command = github::gh_command_with_config(gh_command_path);

		command.args([
			"pr",
			"view",
			pr_url,
			"--json",
			"url,baseRefName,headRefName,headRefOid,state,isDraft,headRepository,headRepositoryOwner",
		]);
		command.current_dir(cwd);

		github::configure_gh_command(&mut command, github_token);

		let output = command
			.output()
			.map_err(|error| format!("Failed to inspect pull request `{pr_url}`: {error}"))?;

		if !output.status.success() {
			let stderr = String::from_utf8_lossy(&output.stderr);

			return Err(format!("Failed to inspect pull request `{pr_url}`: {}", stderr.trim()));
		}

		let response: PullRequestViewResponse =
			serde_json::from_slice(&output.stdout).map_err(|error| {
				format!("Failed to parse pull request details for `{pr_url}`: {error}")
			})?;
		let Some(head_repository) = response.head_repository else {
			return Err(format!(
				"Pull request `{pr_url}` does not expose a head repository for review handoff validation."
			));
		};

		Ok(PullRequestDetails {
			base_ref_name: response.base_ref_name,
			head_ref_name: response.head_ref_name,
			head_ref_oid: response.head_ref_oid,
			head_repository_name: head_repository.name,
			head_repository_owner: response.head_repository_owner.login,
			is_draft: response.is_draft,
			state: response.state,
			url: response.url,
		})
	}
}

#[derive(Debug, Deserialize)]
struct PullRequestViewResponse {
	#[serde(rename = "baseRefName")]
	base_ref_name: String,
	#[serde(rename = "headRefName")]
	head_ref_name: String,
	#[serde(rename = "headRefOid")]
	head_ref_oid: String,
	#[serde(rename = "headRepository")]
	head_repository: Option<PullRequestRepositoryResponse>,
	#[serde(rename = "headRepositoryOwner")]
	head_repository_owner: PullRequestRepositoryOwnerResponse,
	#[serde(rename = "isDraft")]
	is_draft: bool,
	state: String,
	url: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestRepositoryResponse {
	name: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestRepositoryOwnerResponse {
	login: String,
}

pub(in crate::agent::tracker_tool_bridge) fn resolve_review_handoff_github_token(
	review_context: &ReviewHandoffContext,
) -> std::result::Result<String, String> {
	let Some(env_var) = review_context.github_token_env_var.as_deref() else {
		return Err(String::from(
			"`github.token_env_var` must be configured for PR-backed review handoff validation.",
		));
	};
	let value = env::var(env_var).map_err(|error| {
		format!(
			"Failed to read environment variable `{env_var}` referenced by `github.token_env_var`: {error}"
		)
	})?;

	if value.trim().is_empty() {
		return Err(format!(
			"Environment variable `{env_var}` referenced by `github.token_env_var` must not be blank."
		));
	}

	Ok(value)
}
