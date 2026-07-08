use crate::orchestrator::types::Deserialize;

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateResponse {
	pub(crate) data: PullRequestReviewStateData,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateData {
	pub(crate) repository: Option<PullRequestReviewStateRepository>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateRepository {
	#[serde(rename = "mergeCommitAllowed")]
	pub(crate) merge_commit_allowed: bool,
	#[serde(rename = "pullRequest")]
	pub(crate) pull_request: Option<PullRequestReviewStateNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsResponse {
	pub(crate) data: PullRequestIssueCommentsData,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsData {
	pub(crate) repository: Option<PullRequestIssueCommentsRepository>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsRepository {
	#[serde(rename = "pullRequest")]
	pub(crate) pull_request: Option<PullRequestIssueCommentsNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentsNode {
	pub(crate) url: String,
	pub(crate) comments: PullRequestIssueCommentConnection,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewStateNode {
	pub(crate) url: String,
	pub(crate) state: String,
	#[serde(rename = "isDraft")]
	pub(crate) is_draft: bool,
	#[serde(rename = "reviewDecision")]
	pub(crate) review_decision: Option<String>,
	#[serde(rename = "baseRefOid")]
	pub(crate) base_ref_oid: Option<String>,
	#[serde(rename = "reviewRequests")]
	pub(crate) review_requests: PullRequestReviewRequestConnection,
	pub(crate) mergeable: String,
	#[serde(rename = "mergeStateStatus")]
	pub(crate) merge_state_status: String,
	#[serde(rename = "headRefName")]
	pub(crate) head_ref_name: String,
	#[serde(rename = "headRefOid")]
	pub(crate) head_ref_oid: String,
	#[serde(rename = "mergeCommit")]
	pub(crate) merge_commit: Option<PullRequestMergeCommitNode>,
	#[serde(rename = "headRepository")]
	pub(crate) head_repository: Option<PullRequestRepository>,
	#[serde(rename = "headRepositoryOwner")]
	pub(crate) head_repository_owner: Option<PullRequestRepositoryOwner>,
	#[serde(rename = "reactionGroups")]
	pub(crate) reaction_groups: Vec<PullRequestReactionGroup>,
	pub(crate) comments: PullRequestIssueCommentConnection,
	pub(crate) reviews: PullRequestReviewConnection,
	#[serde(rename = "reviewThreads")]
	pub(crate) review_threads: PullRequestReviewThreadConnection,
	pub(crate) commits: PullRequestCommitConnection,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestRepositoryOwner {
	pub(crate) login: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestMergeCommitNode {
	pub(crate) oid: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestRepository {
	pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewRequestConnection {
	#[serde(rename = "totalCount")]
	pub(crate) total_count: usize,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewThreadConnection {
	pub(crate) nodes: Vec<PullRequestReviewThreadNode>,
	#[serde(rename = "pageInfo")]
	pub(crate) page_info: PullRequestPageInfo,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewThreadNode {
	#[serde(rename = "isResolved")]
	pub(crate) is_resolved: bool,
	#[serde(rename = "isOutdated")]
	pub(crate) is_outdated: bool,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentConnection {
	pub(crate) nodes: Vec<PullRequestIssueCommentNode>,
	#[serde(rename = "pageInfo")]
	pub(crate) page_info: PullRequestPageInfo,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestIssueCommentNode {
	#[serde(rename = "databaseId")]
	pub(crate) database_id: i64,
	pub(crate) body: String,
	#[serde(rename = "createdAt")]
	pub(crate) created_at: String,
	pub(crate) author: Option<PullRequestActor>,
	#[serde(rename = "reactionGroups")]
	pub(crate) reaction_groups: Vec<PullRequestReactionGroup>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewConnection {
	pub(crate) nodes: Vec<PullRequestReviewNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReviewNode {
	pub(crate) body: String,
	pub(crate) state: String,
	#[serde(rename = "submittedAt")]
	pub(crate) submitted_at: Option<String>,
	pub(crate) author: Option<PullRequestActor>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReactionGroup {
	pub(crate) content: String,
	pub(crate) users: PullRequestReactionUsersConnection,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestReactionUsersConnection {
	pub(crate) nodes: Vec<PullRequestActor>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestActor {
	pub(crate) login: String,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestPageInfo {
	#[serde(rename = "hasNextPage")]
	pub(crate) has_next_page: bool,
	#[serde(rename = "endCursor")]
	pub(crate) end_cursor: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestCommitConnection {
	pub(crate) nodes: Vec<PullRequestCommitNode>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestCommitNode {
	pub(crate) commit: PullRequestCommitPayload,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestCommitPayload {
	#[serde(rename = "statusCheckRollup")]
	pub(crate) status_check_rollup: Option<PullRequestStatusCheckRollup>,
}

#[derive(Deserialize)]
pub(crate) struct PullRequestStatusCheckRollup {
	pub(crate) state: String,
}
