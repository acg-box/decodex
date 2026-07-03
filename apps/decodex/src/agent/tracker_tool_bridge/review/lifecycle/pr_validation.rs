use crate::agent::tracker_tool_bridge::review::{
	PullRequestDetails, ReviewHandoffContext, TrackerToolBridge, tracker_tool_bridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn validate_review_action_pr(
		&self,
		review_context: &ReviewHandoffContext,
		pr_url: &str,
	) -> std::result::Result<PullRequestDetails, String> {
		let github_token =
			tracker_tool_bridge::resolve_review_handoff_github_token(review_context)?;
		let pull_request = self.pull_request_inspector.inspect_pull_request(
			&review_context.cwd,
			pr_url,
			github_token.as_str(),
			review_context.github_command_path.as_deref(),
		)?;
		let local_repo = self.local_repo_inspector.inspect_local_repo(&review_context.cwd)?;

		if pull_request.head_repository_owner != local_repo.repository_owner
			|| pull_request.head_repository_name != local_repo.repository_name
		{
			return Err(format!(
				"Pull request `{}` belongs to repository `{}/{}`, but the current lane repository is `{}/{}`.",
				pull_request.url,
				pull_request.head_repository_owner,
				pull_request.head_repository_name,
				local_repo.repository_owner,
				local_repo.repository_name
			));
		}
		if pull_request.url != pr_url {
			return Err(format!(
				"Pull request readback returned `{}` while validating requested PR `{}`.",
				pull_request.url, pr_url
			));
		}
		if pull_request.base_ref_name != local_repo.default_branch {
			return Err(format!(
				"Pull request `{}` targets base branch `{}`, but retained review lanes must target the repository default branch `{}`.",
				pull_request.url, pull_request.base_ref_name, local_repo.default_branch
			));
		}
		if pull_request.head_ref_name != review_context.branch_name {
			return Err(format!(
				"Pull request `{}` is for branch `{}`, but the current lane branch is `{}`.",
				pull_request.url, pull_request.head_ref_name, review_context.branch_name
			));
		}
		if pull_request.head_ref_oid != local_repo.head_oid {
			return Err(format!(
				"Pull request `{}` points at commit `{}`, but the current lane HEAD is `{}`. Push the latest lane commit before review handoff.",
				pull_request.url, pull_request.head_ref_oid, local_repo.head_oid
			));
		}
		if pull_request.state != "OPEN" {
			return Err(format!(
				"Pull request `{}` is `{}`; it must be open for review handoff.",
				pull_request.url, pull_request.state
			));
		}
		if pull_request.is_draft {
			return Err(format!(
				"Pull request `{}` is still draft; mark it ready for review before handoff.",
				pull_request.url
			));
		}

		if let Some(recorded_pr_url) = review_context.recorded_pr_url.as_deref()
			&& pull_request.url != recorded_pr_url
		{
			return Err(format!(
				"Pull request `{}` does not match the retained lane PR `{}`.",
				pull_request.url, recorded_pr_url
			));
		}

		Ok(pull_request)
	}

	pub(in crate::agent::tracker_tool_bridge) fn validate_closeout_pr(
		&self,
		review_context: &ReviewHandoffContext,
		pr_url: &str,
	) -> std::result::Result<PullRequestDetails, String> {
		let github_token =
			tracker_tool_bridge::resolve_review_handoff_github_token(review_context)?;
		let pull_request = self.pull_request_inspector.inspect_pull_request(
			&review_context.cwd,
			pr_url,
			github_token.as_str(),
			review_context.github_command_path.as_deref(),
		)?;
		let local_repo = self.local_repo_inspector.inspect_local_repo(&review_context.cwd)?;

		if pull_request.head_repository_owner != local_repo.repository_owner
			|| pull_request.head_repository_name != local_repo.repository_name
		{
			return Err(format!(
				"Pull request `{}` belongs to repository `{}/{}`, but the current lane repository is `{}/{}`.",
				pull_request.url,
				pull_request.head_repository_owner,
				pull_request.head_repository_name,
				local_repo.repository_owner,
				local_repo.repository_name
			));
		}
		if pull_request.base_ref_name != local_repo.default_branch {
			return Err(format!(
				"Pull request `{}` targets base branch `{}`, but retained closeout requires the repository default branch `{}`.",
				pull_request.url, pull_request.base_ref_name, local_repo.default_branch
			));
		}
		if pull_request.head_ref_name != review_context.branch_name {
			return Err(format!(
				"Pull request `{}` is for branch `{}`, but the current lane branch is `{}`.",
				pull_request.url, pull_request.head_ref_name, review_context.branch_name
			));
		}
		if pull_request.head_ref_oid != local_repo.head_oid {
			return Err(format!(
				"Pull request `{}` points at commit `{}`, but the current lane HEAD is `{}`. Finish closeout from the merged lane head.",
				pull_request.url, pull_request.head_ref_oid, local_repo.head_oid
			));
		}
		if pull_request.state != "MERGED" {
			return Err(format!(
				"Pull request `{}` is `{}`; it must be merged before closeout completes.",
				pull_request.url, pull_request.state
			));
		}
		if pull_request.is_draft {
			return Err(format!(
				"Pull request `{}` is still draft; closeout requires a merged non-draft PR lineage.",
				pull_request.url
			));
		}

		if let Some(recorded_pr_url) = review_context.recorded_pr_url.as_deref()
			&& pull_request.url != recorded_pr_url
		{
			return Err(format!(
				"Pull request `{}` does not match the retained lane PR `{}`.",
				pull_request.url, recorded_pr_url
			));
		}

		Ok(pull_request)
	}
}
