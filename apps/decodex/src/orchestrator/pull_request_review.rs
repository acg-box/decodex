mod format;
mod pagination;
mod query;
mod state;

pub(crate) use self::query::{PullRequestIssueCommentsPageQuery, PullRequestReviewStatePageQuery};

use crate::orchestrator::{
	PullRequestIssueCommentConnection, PullRequestIssueCommentNode, PullRequestIssueCommentState,
	PullRequestIssueCommentsNode, PullRequestReactionGroup, PullRequestReviewNode,
	PullRequestReviewState, PullRequestReviewStateNode, PullRequestReviewStateRepository,
	PullRequestReviewSummaryState, PullRequestReviewThreadConnection, Result, RunSummary,
};

pub(crate) fn query_pull_request_review_state_page(
	query: PullRequestReviewStatePageQuery<'_>,
) -> Result<PullRequestReviewStateRepository> {
	query::query_pull_request_review_state_page(query)
}

pub(crate) fn query_pull_request_issue_comments_page(
	query: PullRequestIssueCommentsPageQuery<'_>,
) -> Result<PullRequestIssueCommentsNode> {
	query::query_pull_request_issue_comments_page(query)
}

pub(crate) fn pull_request_review_state_from_page(
	repository: &PullRequestReviewStateRepository,
	pull_request: &PullRequestReviewStateNode,
) -> Result<PullRequestReviewState> {
	state::pull_request_review_state_from_page(repository, pull_request)
}

pub(crate) fn merge_pull_request_review_state_page(
	review_state: &mut PullRequestReviewState,
	repository: &PullRequestReviewStateRepository,
	pull_request: &PullRequestReviewStateNode,
) -> Result<Option<String>> {
	pagination::merge_pull_request_review_state_page(review_state, repository, pull_request)
}

pub(crate) fn merge_pull_request_issue_comment_page(
	review_state: &mut PullRequestReviewState,
	pull_request: &PullRequestIssueCommentsNode,
) -> Result<Option<String>> {
	pagination::merge_pull_request_issue_comment_page(review_state, pull_request)
}

pub(crate) fn count_unresolved_review_threads(
	review_threads: &PullRequestReviewThreadConnection,
) -> usize {
	state::count_unresolved_review_threads(review_threads)
}

pub(crate) fn pull_request_status_check_rollup_state(
	pull_request: &PullRequestReviewStateNode,
) -> Option<String> {
	state::pull_request_status_check_rollup_state(pull_request)
}

pub(crate) fn issue_comment_state_from_node(
	comment: &PullRequestIssueCommentNode,
) -> Result<PullRequestIssueCommentState> {
	state::issue_comment_state_from_node(comment)
}

pub(crate) fn review_summary_state_from_node(
	review: &PullRequestReviewNode,
) -> Option<PullRequestReviewSummaryState> {
	state::review_summary_state_from_node(review)
}

pub(crate) fn reaction_group_actor_count(
	groups: &[PullRequestReactionGroup],
	content: &str,
	actor_login: &str,
) -> usize {
	state::reaction_group_actor_count(groups, content, actor_login)
}

pub(crate) fn next_pull_request_review_threads_cursor(
	pull_request: &PullRequestReviewStateNode,
) -> Result<Option<String>> {
	pagination::next_pull_request_review_threads_cursor(pull_request)
}

pub(crate) fn next_pull_request_issue_comments_cursor(
	comments: &PullRequestIssueCommentConnection,
	pr_url: &str,
) -> Result<Option<String>> {
	pagination::next_pull_request_issue_comments_cursor(comments, pr_url)
}

pub(crate) fn format_run_once_summary(summary: &RunSummary, dry_run: bool) -> String {
	format::format_run_once_summary(summary, dry_run)
}
