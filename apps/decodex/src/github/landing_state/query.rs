use std::path::Path;

use crate::{
	github,
	github::landing_state::model::{PullRequestLandingStateNode, PullRequestLandingStateResponse},
	prelude::{Result, eyre},
};

const PULL_REQUEST_LANDING_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
      baseRefName
      baseRefOid
      mergeable
      mergeStateStatus
      headRefName
      headRefOid
      reviewRequests(first: 1) {
        totalCount
      }
      reviewThreads(first: 100, after: $reviewThreadsAfter) {
        nodes {
          isResolved
          isOutdated
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }
    }
  }
}
"#;

pub(in crate::github::landing_state) struct PullRequestLandingStatePageQuery<'a> {
	pub(in crate::github::landing_state) cwd: &'a Path,
	pub(in crate::github::landing_state) owner: &'a str,
	pub(in crate::github::landing_state) repo: &'a str,
	pub(in crate::github::landing_state) number: u64,
	pub(in crate::github::landing_state) review_threads_after: Option<&'a str>,
	pub(in crate::github::landing_state) pr_url: &'a str,
	pub(in crate::github::landing_state) github_token: &'a str,
	pub(in crate::github::landing_state) gh_command_path: Option<&'a Path>,
}

pub(in crate::github::landing_state) fn query_pull_request_landing_state_page(
	query: PullRequestLandingStatePageQuery<'_>,
) -> Result<PullRequestLandingStateNode> {
	let mut command = github::gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_LANDING_STATE_QUERY}")]);
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
			"Failed to inspect pull request landing state `{}`: {}",
			query.pr_url,
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<PullRequestLandingStateResponse>(&output.stdout)?;
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
