use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestLandingStateResponse {
	pub(in crate::github::landing_state) data: PullRequestLandingStateData,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestLandingStateData {
	pub(in crate::github::landing_state) repository: Option<PullRequestLandingStateRepository>,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestLandingStateRepository {
	#[serde(rename = "pullRequest")]
	pub(in crate::github::landing_state) pull_request: Option<PullRequestLandingStateNode>,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestLandingStateNode {
	pub(in crate::github::landing_state) url: String,
	pub(in crate::github::landing_state) state: String,
	#[serde(rename = "isDraft")]
	pub(in crate::github::landing_state) is_draft: bool,
	#[serde(rename = "reviewDecision")]
	pub(in crate::github::landing_state) review_decision: Option<String>,
	#[serde(rename = "baseRefName")]
	pub(in crate::github::landing_state) base_ref_name: String,
	#[serde(rename = "baseRefOid")]
	pub(in crate::github::landing_state) base_ref_oid: Option<String>,
	#[serde(rename = "mergeable")]
	pub(in crate::github::landing_state) mergeable: String,
	#[serde(rename = "mergeStateStatus")]
	pub(in crate::github::landing_state) merge_state_status: String,
	#[serde(rename = "headRefName")]
	pub(in crate::github::landing_state) head_ref_name: String,
	#[serde(rename = "headRefOid")]
	pub(in crate::github::landing_state) head_ref_oid: String,
	#[serde(rename = "reviewRequests")]
	pub(in crate::github::landing_state) review_requests: PullRequestReviewRequestConnection,
	#[serde(rename = "reviewThreads")]
	pub(in crate::github::landing_state) review_threads: PullRequestReviewThreadConnection,
	pub(in crate::github::landing_state) commits: PullRequestCommitConnection,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestReviewRequestConnection {
	#[serde(rename = "totalCount")]
	pub(in crate::github::landing_state) total_count: usize,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestReviewThreadConnection {
	pub(in crate::github::landing_state) nodes: Vec<PullRequestReviewThreadNode>,
	#[serde(rename = "pageInfo")]
	pub(in crate::github::landing_state) page_info: PullRequestPageInfo,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestReviewThreadNode {
	#[serde(rename = "isResolved")]
	pub(in crate::github::landing_state) is_resolved: bool,
	#[serde(rename = "isOutdated")]
	pub(in crate::github::landing_state) is_outdated: bool,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestPageInfo {
	#[serde(rename = "hasNextPage")]
	pub(in crate::github::landing_state) has_next_page: bool,
	#[serde(rename = "endCursor")]
	pub(in crate::github::landing_state) end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestCommitConnection {
	pub(in crate::github::landing_state) nodes: Vec<PullRequestCommitNode>,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestCommitNode {
	pub(in crate::github::landing_state) commit: PullRequestCommitPayload,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestCommitPayload {
	#[serde(rename = "statusCheckRollup")]
	pub(in crate::github::landing_state) status_check_rollup: Option<PullRequestStatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
pub(in crate::github::landing_state) struct PullRequestStatusCheckRollup {
	pub(in crate::github::landing_state) state: String,
}
