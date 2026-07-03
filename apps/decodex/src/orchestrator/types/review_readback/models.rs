#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestReviewState {
	pub(crate) url: String,
	pub(crate) state: String,
	pub(crate) is_draft: bool,
	pub(crate) review_decision: Option<String>,
	pub(crate) merge_commit_allowed: bool,
	pub(crate) pending_review_requests: usize,
	pub(crate) mergeable: String,
	pub(crate) merge_state_status: String,
	pub(crate) head_ref_name: String,
	pub(crate) head_ref_oid: String,
	pub(crate) merge_commit_oid: Option<String>,
	pub(crate) head_repository_name: Option<String>,
	pub(crate) head_repository_owner: Option<String>,
	pub(crate) status_check_rollup_state: Option<String>,
	pub(crate) unresolved_review_threads: usize,
	pub(crate) issue_description_external_review_thumbs_up_count: usize,
	pub(crate) issue_comments: Vec<PullRequestIssueCommentState>,
	pub(crate) reviews: Vec<PullRequestReviewSummaryState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestIssueCommentState {
	pub(crate) database_id: i64,
	pub(crate) author_login: Option<String>,
	pub(crate) body: String,
	pub(crate) created_at_unix_epoch: i64,
	pub(crate) external_review_eyes_reaction_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestReviewSummaryState {
	pub(crate) author_login: Option<String>,
	pub(crate) body: String,
	pub(crate) state: String,
	pub(crate) submitted_at_unix_epoch: i64,
}
