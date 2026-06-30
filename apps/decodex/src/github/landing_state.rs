use std::path::Path;

use serde::Deserialize;

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

use super::{configure_gh_command, gh_command_with_config, parse_pull_request_url};

const PULL_REQUEST_LANDING_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
      baseRefName
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
#[derive(Debug, Deserialize)]
struct PullRequestLandingStateResponse {
	data: PullRequestLandingStateData,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateData {
	repository: Option<PullRequestLandingStateRepository>,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateRepository {
	#[serde(rename = "pullRequest")]
	pull_request: Option<PullRequestLandingStateNode>,
}

#[derive(Debug, Deserialize)]
struct PullRequestLandingStateNode {
	url: String,
	state: String,
	#[serde(rename = "isDraft")]
	is_draft: bool,
	#[serde(rename = "reviewDecision")]
	review_decision: Option<String>,
	#[serde(rename = "baseRefName")]
	base_ref_name: String,
	#[serde(rename = "mergeable")]
	mergeable: String,
	#[serde(rename = "mergeStateStatus")]
	merge_state_status: String,
	#[serde(rename = "headRefName")]
	head_ref_name: String,
	#[serde(rename = "headRefOid")]
	head_ref_oid: String,
	#[serde(rename = "reviewRequests")]
	review_requests: PullRequestReviewRequestConnection,
	#[serde(rename = "reviewThreads")]
	review_threads: PullRequestReviewThreadConnection,
	commits: PullRequestCommitConnection,
}

struct PullRequestLandingStatePageQuery<'a> {
	cwd: &'a Path,
	owner: &'a str,
	repo: &'a str,
	number: u64,
	review_threads_after: Option<&'a str>,
	pr_url: &'a str,
	github_token: &'a str,
	gh_command_path: Option<&'a Path>,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviewRequestConnection {
	#[serde(rename = "totalCount")]
	total_count: usize,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviewThreadConnection {
	nodes: Vec<PullRequestReviewThreadNode>,
	#[serde(rename = "pageInfo")]
	page_info: PullRequestPageInfo,
}

#[derive(Debug, Deserialize)]
struct PullRequestReviewThreadNode {
	#[serde(rename = "isResolved")]
	is_resolved: bool,
	#[serde(rename = "isOutdated")]
	is_outdated: bool,
}

#[derive(Debug, Deserialize)]
struct PullRequestPageInfo {
	#[serde(rename = "hasNextPage")]
	has_next_page: bool,
	#[serde(rename = "endCursor")]
	end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PullRequestCommitConnection {
	nodes: Vec<PullRequestCommitNode>,
}

#[derive(Debug, Deserialize)]
struct PullRequestCommitNode {
	commit: PullRequestCommitPayload,
}

#[derive(Debug, Deserialize)]
struct PullRequestCommitPayload {
	#[serde(rename = "statusCheckRollup")]
	status_check_rollup: Option<PullRequestStatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
struct PullRequestStatusCheckRollup {
	state: String,
}

pub(crate) fn inspect_pull_request_landing_state(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestLandingState> {
	let locator = parse_pull_request_url(pr_url)?;
	let mut review_threads_after: Option<String> = None;
	let mut landing_state: Option<PullRequestLandingState> = None;

	loop {
		let pull_request =
			query_pull_request_landing_state_page(PullRequestLandingStatePageQuery {
				cwd,
				owner: &locator.owner,
				repo: &locator.repo,
				number: locator.number,
				review_threads_after: review_threads_after.as_deref(),
				pr_url,
				github_token,
				gh_command_path,
			})?;
		let next_cursor = match &mut landing_state {
			Some(landing_state) =>
				merge_pull_request_landing_state_page(landing_state, &pull_request)?,
			None => {
				let next_cursor = next_pull_request_review_threads_cursor(&pull_request, pr_url)?;

				landing_state = Some(pull_request_landing_state_from_page(&pull_request));

				next_cursor
			},
		};
		let Some(next_cursor) = next_cursor else {
			break;
		};

		review_threads_after = Some(next_cursor);
	}

	landing_state.ok_or_else(|| {
		eyre::eyre!("GitHub GraphQL response for `{pr_url}` did not include a pull request.")
	})
}

fn query_pull_request_landing_state_page(
	query: PullRequestLandingStatePageQuery<'_>,
) -> Result<PullRequestLandingStateNode> {
	let mut command = gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_LANDING_STATE_QUERY}")]);
	command.args(["-F", &format!("owner={}", query.owner)]);
	command.args(["-F", &format!("name={}", query.repo)]);
	command.args(["-F", &format!("number={}", query.number)]);

	if let Some(review_threads_after) = query.review_threads_after {
		command.args(["-F", &format!("reviewThreadsAfter={review_threads_after}")]);
	}

	command.current_dir(query.cwd);

	configure_gh_command(&mut command, query.github_token);

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

fn pull_request_landing_state_from_page(
	pull_request: &PullRequestLandingStateNode,
) -> PullRequestLandingState {
	PullRequestLandingState {
		url: pull_request.url.clone(),
		state: pull_request.state.clone(),
		is_draft: pull_request.is_draft,
		review_decision: pull_request.review_decision.clone(),
		base_ref_name: pull_request.base_ref_name.clone(),
		pending_review_requests: pull_request.review_requests.total_count,
		mergeable: pull_request.mergeable.clone(),
		merge_state_status: pull_request.merge_state_status.clone(),
		head_ref_name: pull_request.head_ref_name.clone(),
		head_ref_oid: pull_request.head_ref_oid.clone(),
		status_check_rollup_state: pull_request
			.commits
			.nodes
			.first()
			.and_then(|node| node.commit.status_check_rollup.as_ref())
			.map(|rollup| rollup.state.clone()),
		unresolved_review_threads: count_unresolved_review_threads(&pull_request.review_threads),
	}
}

fn merge_pull_request_landing_state_page(
	landing_state: &mut PullRequestLandingState,
	pull_request: &PullRequestLandingStateNode,
) -> Result<Option<String>> {
	let page_state = pull_request_landing_state_from_page(pull_request);

	if landing_state.url != page_state.url
		|| landing_state.state != page_state.state
		|| landing_state.is_draft != page_state.is_draft
		|| landing_state.review_decision != page_state.review_decision
		|| landing_state.base_ref_name != page_state.base_ref_name
		|| landing_state.pending_review_requests != page_state.pending_review_requests
		|| landing_state.mergeable != page_state.mergeable
		|| landing_state.merge_state_status != page_state.merge_state_status
		|| landing_state.head_ref_name != page_state.head_ref_name
		|| landing_state.head_ref_oid != page_state.head_ref_oid
		|| landing_state.status_check_rollup_state != page_state.status_check_rollup_state
	{
		eyre::bail!("Pull request landing state changed while paginating `{}`.", landing_state.url);
	}

	landing_state.unresolved_review_threads += page_state.unresolved_review_threads;

	next_pull_request_review_threads_cursor(pull_request, landing_state.url.as_str())
}

fn count_unresolved_review_threads(review_threads: &PullRequestReviewThreadConnection) -> usize {
	review_threads.nodes.iter().filter(|thread| !thread.is_resolved && !thread.is_outdated).count()
}

fn next_pull_request_review_threads_cursor(
	pull_request: &PullRequestLandingStateNode,
	pr_url: &str,
) -> Result<Option<String>> {
	if !pull_request.review_threads.page_info.has_next_page {
		return Ok(None);
	}

	pull_request
		.review_threads
		.page_info
		.end_cursor
		.clone()
		.map(Some)
		.ok_or_else(|| {
			eyre::eyre!(
				"GitHub GraphQL response for `{pr_url}` reported additional review thread pages without an end cursor."
			)
		})
}
