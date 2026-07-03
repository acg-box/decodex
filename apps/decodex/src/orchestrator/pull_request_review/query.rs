use crate::orchestrator::{
	PULL_REQUEST_ISSUE_COMMENTS_QUERY, PULL_REQUEST_REVIEW_STATE_QUERY, Path,
	PullRequestIssueCommentsNode, PullRequestIssueCommentsResponse,
	PullRequestReviewStateRepository, PullRequestReviewStateResponse, Result, eyre, github,
};

pub(crate) struct PullRequestReviewStatePageQuery<'a> {
	pub(crate) cwd: &'a Path,
	pub(crate) owner: &'a str,
	pub(crate) repo: &'a str,
	pub(crate) number: u64,
	pub(crate) review_threads_after: Option<&'a str>,
	pub(crate) pr_url: &'a str,
	pub(crate) github_token: &'a str,
	pub(crate) gh_command_path: Option<&'a Path>,
}

pub(crate) struct PullRequestIssueCommentsPageQuery<'a> {
	pub(crate) cwd: &'a Path,
	pub(crate) owner: &'a str,
	pub(crate) repo: &'a str,
	pub(crate) number: u64,
	pub(crate) comments_after: &'a str,
	pub(crate) pr_url: &'a str,
	pub(crate) github_token: &'a str,
	pub(crate) gh_command_path: Option<&'a Path>,
}

pub(crate) fn query_pull_request_review_state_page(
	query: PullRequestReviewStatePageQuery<'_>,
) -> Result<PullRequestReviewStateRepository> {
	let mut command = github::gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_REVIEW_STATE_QUERY}")]);
	command.args(["-F", &format!("owner={}", query.owner)]);
	command.args(["-F", &format!("name={}", query.repo)]);
	command.args(["-F", &format!("number={}", query.number)]);

	if let Some(review_threads_after) = query.review_threads_after {
		command.args(["-F", &format!("reviewThreadsAfter={review_threads_after}")]);
	}

	command.current_dir(query.cwd);

	github::configure_gh_command(&mut command, query.github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect pull request review state `{}`: {}",
			query.pr_url,
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<PullRequestReviewStateResponse>(&output.stdout)?;
	let Some(repository) = response.data.repository else {
		eyre::bail!("GitHub GraphQL response for `{}` did not include a repository.", query.pr_url);
	};

	if repository.pull_request.is_none() {
		eyre::bail!(
			"GitHub GraphQL response for `{}` did not include a pull request.",
			query.pr_url
		);
	}

	Ok(repository)
}

pub(crate) fn query_pull_request_issue_comments_page(
	query: PullRequestIssueCommentsPageQuery<'_>,
) -> Result<PullRequestIssueCommentsNode> {
	let mut command = github::gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_ISSUE_COMMENTS_QUERY}")]);
	command.args(["-F", &format!("owner={}", query.owner)]);
	command.args(["-F", &format!("name={}", query.repo)]);
	command.args(["-F", &format!("number={}", query.number)]);
	command.args(["-F", &format!("commentsAfter={}", query.comments_after)]);
	command.current_dir(query.cwd);

	github::configure_gh_command(&mut command, query.github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect pull request issue comments for `{}`: {}",
			query.pr_url,
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<PullRequestIssueCommentsResponse>(&output.stdout)?;
	let Some(repository) = response.data.repository else {
		eyre::bail!("GitHub GraphQL response for `{}` did not include a repository.", query.pr_url);
	};
	let Some(pull_request) = repository.pull_request else {
		eyre::bail!(
			"GitHub GraphQL response for `{}` did not include a pull request.",
			query.pr_url
		);
	};

	Ok(pull_request)
}
